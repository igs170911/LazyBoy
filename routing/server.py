"""Local hints only. No prompts in logs, no remote encoders, no inference queue."""
import asyncio
from contextlib import asynccontextmanager
import json
import math
import os
from pathlib import Path
import time
from fastapi import FastAPI, HTTPException
from fastapi.responses import JSONResponse
from pydantic import BaseModel, Field

ROUTES = json.loads(Path(__file__).with_name("routes.json").read_text())
QUESTIONS = {"route": {"type": "choice", "instructions":
    "Which capability is needed for this request? Supplied text can be translated or explained directly. Current facts and external actions require tools.",
    "criteria": {
        "answer": "answer, translate, rewrite or explain using only the supplied context",
        "native": "current information, search, files, commands or connected APIs",
        "browser": "website login, web forms or interactive JavaScript page",
        "desktop": "screenshot or native desktop application interaction",
        "work": "complex implementation, multi-step verification or deep research"}}}


def confident_choice(answer, floor):
    # New Laya uses probability of the top answer; entropy confidence is legacy.
    score = answer.get("answer_confidence", answer.get("confidence"))
    try:
        score = float(score)
    except (TypeError, ValueError):
        return None
    label = answer.get("choice", answer.get("answer"))
    if answer.get("abstained") or not math.isfinite(score) or score < floor or label not in ROUTES:
        return None
    return label, score


class Engines:
    def __init__(self):
        self.semantic = self.laya = None
        self.status = {"semantic": "warming", "laya": "warming"}
        self.backend = os.getenv("LAYA_BACKEND", "auto")
        self.busy = False
        self.tasks = set()
        self.thresholds = {}
        calibration = Path(os.getenv("ROUTER_CALIBRATION", "/models/router-calibration.json"))
        if calibration.exists():
            try:
                artifact = json.loads(calibration.read_text())
                if artifact.get("version") == 1 and artifact.get("source") == "independent labeled corpora":
                    self.thresholds = artifact["thresholds"]
            except (OSError, ValueError, KeyError):
                pass

    def warm(self):
        import torch
        torch.set_num_threads(int(os.getenv("LAYA_THREADS", "2")))
        torch.set_num_interop_threads(1)
        try:
            from semantic_router import Route, SemanticRouter
            from semantic_router.encoders import FastEmbedEncoder
            encoder = FastEmbedEncoder(name="sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2",
                threads=2, cache_dir="/models/fastembed", score_threshold=0.5)
            self.semantic = SemanticRouter(encoder=encoder, routes=[Route(name=k, utterances=v)
                for k, v in ROUTES.items()], auto_sync="local", top_k=5, aggregation="max")
            self.semantic("你好")
            self.status["semantic"] = "ready"
        except Exception as error:
            self.status["semantic"] = "failed:" + type(error).__name__
        requested_backend = self.backend
        if self.backend == "auto":
            self.backend = "onnx" if Path(os.getenv("LAYA_ONNX_PATH", "/models/laya.onnx")).exists() else "torch"
        try:
            if self.backend == "onnx":
                import onnxruntime as ort
                original_options = ort.SessionOptions
                def limited_options():
                    options = original_options()
                    options.intra_op_num_threads = 2
                    options.inter_op_num_threads = 1
                    return options
                ort.SessionOptions = limited_options
                from laya.onnx_agent import ONNXAgent
                self.laya = ONNXAgent(os.getenv("LAYA_MODEL_PATH", "convaiinnovations/laya-multilingual"),
                    onnx_path=os.getenv("LAYA_ONNX_PATH", "/models/laya.onnx"))
                ort.SessionOptions = original_options
            elif self.backend == "openvino":
                from laya import OVAgent
                from huggingface_hub import snapshot_download
                model_path = os.getenv("LAYA_MODEL_PATH", "/models/laya-ov-int8")
                if not Path(model_path, "rl_agent_config.json").exists():
                    snapshot_download("rupeshs/laya-ov-int8", local_dir=model_path)
                self.laya = OVAgent(model_path, device="CPU",
                    ov_config={"PERFORMANCE_HINT": "LATENCY", "INFERENCE_NUM_THREADS": "2"})
            elif self.backend == "torch":
                from laya import Agent
                self.laya = Agent(os.getenv("LAYA_MODEL_PATH", "convaiinnovations/laya-multilingual"), device="cpu")
            else:
                raise ValueError("unsupported backend")
            self.predict_laya("Hello" if self.backend == "openvino" else "你好", QUESTIONS)
            self.status["laya"] = "ready"
        except Exception as error:
            if "original_options" in locals():
                ort.SessionOptions = original_options
            self.status["laya"] = "failed:" + type(error).__name__
            if requested_backend == "auto" and self.backend == "onnx":
                try:
                    from laya import Agent
                    self.backend = "torch"
                    self.laya = Agent(os.getenv("LAYA_MODEL_PATH", "convaiinnovations/laya-multilingual"), device="cpu")
                    self.predict_laya("你好", QUESTIONS)
                    self.status["laya"] = "ready"
                except Exception:
                    pass

    def predict_laya(self, state, questions):
        if self.backend == "openvino":
            return self.laya.system_one(state, questions)
        return self.laya.system_one(state, questions, max_len=256)

    async def run(self, function, deadline_ms):
        # One event-loop owner; timed out native work retains its slot until it exits.
        if self.busy:
            return None, "busy"
        self.busy = True
        task = asyncio.create_task(asyncio.to_thread(function))
        self.tasks.add(task)
        def finished(task):
            self.busy = False
            self.tasks.discard(task)
            if not task.cancelled():
                task.exception()  # retrieve without logging prompt-bearing exceptions
        task.add_done_callback(finished)
        try:
            return await asyncio.wait_for(asyncio.shield(task), deadline_ms / 1000), None
        except asyncio.TimeoutError:
            return None, "timeout"
        except Exception:
            return None, "inference_error"

    def acceptance_floor(self, source, route, requested):
        try:
            floor = float(self.thresholds.get(source, {}).get(route, requested))
            return max(requested, floor) if math.isfinite(floor) and 0.5 <= floor <= 2 else 2
        except (TypeError, ValueError):
            return 2

    def route(self, request, started):
        deadline = started + request.deadline_ms / 1000
        if request.semantic and self.status["semantic"] == "ready":
            choices = self.semantic(request.text, limit=2)
            if not isinstance(choices, list):
                choices = [choices]
            choices.sort(key=lambda c: c.similarity_score or 0, reverse=True)
            if choices:
                top = choices[0]
                score = top.similarity_score or 0
                runner = choices[1].similarity_score or 0 if len(choices) > 1 else 0
                if top.name in ROUTES and score >= self.acceptance_floor("semantic", top.name, request.min_confidence) and score - runner >= 0.08:
                    return {"route": top.name, "confidence": score, "source": "semantic", "score_kind": "cosine"}
        if time.perf_counter() >= deadline:
            return {"abstained": True, "reason": "timeout"}
        if request.laya and self.status["laya"] == "ready":
            # The unofficial OpenVINO fork only supports the English checkpoint.
            if self.backend == "openvino" and not request.text.isascii():
                return {"abstained": True, "reason": "unsupported_language"}
            body = self.predict_laya(request.text, QUESTIONS)
            choice = confident_choice(body.get("answers", {}).get("route", {}), request.min_confidence)
            if choice and choice[1] >= self.acceptance_floor("laya", choice[0], request.min_confidence):
                return {"route": choice[0], "confidence": choice[1], "source": "laya", "score_kind": "answer_probability"}
        return {"abstained": True, "reason": "uncertain"}


engines = Engines()

@asynccontextmanager
async def lifespan(app):
    warm = asyncio.create_task(asyncio.to_thread(engines.warm))
    yield
    await warm
    if engines.tasks:
        await asyncio.gather(*engines.tasks, return_exceptions=True)

app = FastAPI(lifespan=lifespan)

class RouteRequest(BaseModel):
    text: str = Field(min_length=1, max_length=2000)
    semantic: bool = True
    laya: bool = True
    min_confidence: float = Field(default=0.9, ge=0.5, le=1)
    deadline_ms: int = Field(default=250, ge=20, le=1000)

@app.get("/health")
async def health():
    ready = all(v == "ready" for v in engines.status.values())
    return JSONResponse({"ready": ready, "engines": engines.status, "backend": engines.backend,
        "busy": engines.busy}, status_code=200 if ready else 503)

@app.post("/v1/route")
async def route(request: RouteRequest):
    started = time.perf_counter()
    result, reason = await engines.run(lambda: engines.route(request, started), request.deadline_ms)
    return {**(result or {"abstained": True, "reason": reason}),
        "elapsed_ms": round((time.perf_counter() - started) * 1000, 2)}

class SecretRequest(BaseModel):
    state: str = Field(min_length=1, max_length=2000)
    questions: dict
    model: str = "multilingual"

@app.post("/v1/systemone")
async def systemone(request: SecretRequest):
    if engines.status["laya"] != "ready":
        raise HTTPException(503, "Laya is warming or unavailable")
    if engines.backend == "openvino" and not request.state.isascii():
        raise HTTPException(422, "OpenVINO checkpoint only supports English")
    result, reason = await engines.run(lambda: engines.predict_laya(request.state, request.questions), 250)
    if result is None:
        raise HTTPException(503, reason)
    return result
