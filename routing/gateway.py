"""Loopback LiteLLM gateway: latency routing, affinity, cooldown and fallback."""
from contextlib import asynccontextmanager
import hmac
import json
import os
from pathlib import Path
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import StreamingResponse

router = None
master_key = ""

@asynccontextmanager
async def lifespan(app):
    global router, master_key
    from litellm import Router
    config = json.loads(Path(os.getenv("GATEWAY_CONFIG", "/config/gateway.json")).read_text())
    master_key = config["general_settings"]["master_key"]
    if not master_key or not config["model_list"]:
        raise RuntimeError("gateway requires a key and at least one configured deployment")
    router = Router(model_list=config["model_list"], **config["router_settings"])
    yield
    router.reset()

app = FastAPI(lifespan=lifespan)

@app.get("/health")
async def health():
    return {"ready": router is not None}

def upstream_finished(stream, depth=0):
    """Inspect pinned LiteLLM wrappers, which synthesize a stop chunk at EOF."""
    if depth > 8:
        return False
    generator = getattr(stream, "_async_generator", None)
    frame = getattr(generator, "ag_frame", None)
    if frame is not None:
        inner = frame.f_locals.get("fallback_response") or frame.f_locals.get("model_response")
        if inner is not None and inner is not stream:
            return upstream_finished(inner, depth + 1)
    return getattr(stream, "received_finish_reason", None) is not None

@app.post("/v1/chat/completions")
async def completion(request: Request):
    if not hmac.compare_digest(request.headers.get("authorization", ""), "Bearer " + master_key):
        raise HTTPException(401, "invalid gateway key")
    body = await request.json()
    if body.get("model") not in {item["model_name"] for item in router.model_list}:
        raise HTTPException(400, "unknown model alias")
    # Only allow generation options; callers cannot override router credentials,
    # retry policy, deployments, or fallback chains through the request body.
    allowed = {"model", "messages", "tools", "tool_choice", "stream", "temperature", "max_tokens", "max_completion_tokens", "reasoning_effort", "parallel_tool_calls"}
    args = {key: value for key, value in body.items() if key in allowed}
    args["metadata"] = {"session_id": request.headers.get("x-litellm-session-id", "")}
    try:
        reply = await router.acompletion(**args)
    except Exception as error:
        status = getattr(error, "status_code", 502)
        if not isinstance(status, int) or not 400 <= status <= 599:
            status = 502
        raise HTTPException(status, "upstream model unavailable") from None
    if not args.get("stream"):
        return reply.model_dump(exclude_none=True)
    async def events():
        stream_id = None
        completed = False
        try:
            async for chunk in reply:
                # A router may swap a stream before any output; after a chunk
                # was delivered, changing stream IDs must terminate, not replay.
                current = getattr(chunk, "id", None)
                if stream_id is not None and current and current != stream_id:
                    return
                if current:
                    stream_id = current
                terminal = any(getattr(choice, "finish_reason", None) is not None for choice in getattr(chunk, "choices", []))
                # LiteLLM 1.104.0 synthesizes finish_reason=stop at bare EOF.
                # The pinned wrapper records real upstream finish reasons separately.
                if terminal and not upstream_finished(reply):
                    return
                completed |= terminal
                yield "data: " + chunk.model_dump_json(exclude_none=True) + "\n\n"
            if completed:
                yield "data: [DONE]\n\n"
        except Exception:
            return  # missing terminal event makes the Rust adapter reject it
        finally:
            try:
                await reply.aclose()
            except Exception:
                pass
    return StreamingResponse(events(), media_type="text/event-stream")
