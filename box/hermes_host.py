"""Authenticated transport host for the unmodified upstream Hermes gateway."""
import hmac
import json
import faulthandler
import fcntl
import os
from pathlib import Path
import re
import signal
import sqlite3

from fastapi import FastAPI, WebSocket, Request
from fastapi.responses import StreamingResponse, JSONResponse
import httpx
import uvicorn

app = FastAPI(docs_url=None, redoc_url=None)


def prepare_host_state(home: Path, namespace: str | None = None):
    """Hold this Bot home exclusively and retire leases from its previous container."""
    lock = (home / ".product-host.lock").open("a+")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        namespace = namespace or re.search(r"\[(\d+)\]", os.readlink("/proc/self/ns/pid")).group(1)
        stamp = home / ".product-host-namespace"
        raw = stamp.read_text().strip() if stamp.exists() else ""
        try: previous = {"namespace": raw, "pid": None} if raw.isdecimal() else json.loads(raw) if raw else {}
        except json.JSONDecodeError: previous = {}
        if not isinstance(previous, dict): previous = {}
        previous_namespace = str(previous.get("namespace", ""))
        database = home / "state.db"
        if previous_namespace.isdecimal() and database.exists():
            with sqlite3.connect(database, timeout=10) as connection:
                for table in ("session_turn_leases", "compression_locks"):
                    if not connection.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?", (table,)).fetchone():
                        continue
                    for rowid, holder in connection.execute(f"SELECT rowid,holder FROM {table}").fetchall():
                        recorded = re.search(r"(?:^|:)pidns=(\d+)(?::|$)", holder)
                        owner = re.search(r"(?:^|:)pid=(\d+)(?::|$)", holder)
                        belongs_to_previous = (previous_namespace != namespace or (owner and int(owner.group(1)) == previous.get("pid")))
                        if recorded and recorded.group(1) == previous_namespace and belongs_to_previous:
                            connection.execute(f"DELETE FROM {table} WHERE rowid=? AND holder=?", (rowid, holder))
        temporary = stamp.with_suffix(".new")
        temporary.write_text(json.dumps({"namespace":namespace,"pid":os.getpid()}) + "\n"); temporary.chmod(0o600)
        temporary.replace(stamp)
        return lock
    except BaseException:
        lock.close()
        raise


async def relay(path: str, request: Request, *, model_api: bool = False):
    key = Path(os.environ["HERMES_HOME"], ".worker-key").read_text().strip()
    authorization = request.headers.get("authorization", "")
    if model_api and not authorization and request.headers.get("x-api-key"):
        authorization = "Bearer " + request.headers["x-api-key"]
    if not hmac.compare_digest(authorization, "Bearer " + key):
        return JSONResponse({"error": "Bot authentication required"}, status_code=401)
    client = httpx.AsyncClient(transport=httpx.AsyncHTTPTransport(uds="/product-ipc/product.sock"), timeout=httpx.Timeout(300, connect=10))
    headers = {"Authorization": "Bearer " + key, "content-type": request.headers.get("content-type", "application/json")}
    if model_api:
        for name in ("anthropic-beta", "anthropic-version"):
            if request.headers.get(name): headers[name] = request.headers[name]
    try:
        upstream = await client.send(client.build_request(request.method, "http://product" + path, params=request.query_params, headers=headers, content=await request.body()), stream=True)
    except BaseException as error:
        await client.aclose()
        if isinstance(error, httpx.HTTPError):
            return JSONResponse({"error": "Product service unavailable"}, status_code=502)
        raise
    async def chunks():
        try:
            async for chunk in upstream.aiter_bytes(): yield chunk
        finally: await upstream.aclose(); await client.aclose()
    return StreamingResponse(chunks(), status_code=upstream.status_code, media_type=upstream.headers.get("content-type", "application/json"))


@app.api_route("/model/{provider}/v1/{path:path}", methods=["GET", "POST"])
async def model(provider: str, path: str, request: Request):
    return await relay(f"/internal/providers/{provider}/v1/{path}", request, model_api=True)


@app.post("/product")
async def product(request: Request): return await relay("/internal/product", request)


@app.get("/health")
def health():
    return {"ok": True, "engine": "hermes", "desktop": Path("/tmp/lazyboy/ready").exists()}


@app.websocket("/rpc")
async def rpc(socket: WebSocket):
    key = Path(os.environ["HERMES_HOME"], ".worker-key").read_text().strip()
    if not hmac.compare_digest(socket.headers.get("authorization", ""), "Bearer " + key):
        await socket.close(code=1008)
        return
    from tui_gateway.ws import handle_ws
    await handle_ws(socket, auth_identity={"user_id": "lazyhermes-owner"})


if __name__ == "__main__":
    host_lock = prepare_host_state(Path(os.environ["HERMES_HOME"]))
    faulthandler.register(signal.SIGUSR1, all_threads=True)
    uvicorn.run(app, host="0.0.0.0", port=8642, log_level="warning")
