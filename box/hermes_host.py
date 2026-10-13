"""Authenticated transport host for the unmodified upstream Hermes gateway."""
import asyncio
from contextlib import asynccontextmanager
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

def redundant_wakeup(context):
    """anyio's UNIX socket waits wake a Future from a reader callback and remove the reader only in a
    later done-callback; a socket still readable in between (EOF after a model reply over the product
    socket) runs the reader again, and that second set_result on the finished Future raises
    InvalidStateError. The waiter already resumed, so nothing is lost: it is noise, not a failure."""
    callback = getattr(context.get("handle"), "_callback", None)
    future = getattr(callback, "__self__", None)
    return (isinstance(context.get("exception"), asyncio.InvalidStateError) and getattr(callback, "__name__", "") == "set_result"
            and isinstance(future, asyncio.Future) and future.done())


def quiet_redundant_wakeups(loop, context):
    if not redundant_wakeup(context): loop.default_exception_handler(context)


@asynccontextmanager
async def lifespan(app):
    asyncio.get_running_loop().set_exception_handler(quiet_redundant_wakeups)
    yield


app = FastAPI(docs_url=None, redoc_url=None, lifespan=lifespan)


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


_product = None
_key = None


def product_client():
    """One client for every relayed request: building one costs an SSL context and a connection each time."""
    global _product
    if _product is None or _product.is_closed:
        _product = httpx.AsyncClient(transport=httpx.AsyncHTTPTransport(uds="/product-ipc/product.sock"), timeout=httpx.Timeout(300, connect=10))
    return _product


def worker_key():
    global _key
    if _key is None: _key = Path(os.environ["HERMES_HOME"], ".worker-key").read_text().strip()
    return _key


async def relay(path: str, request: Request, *, model_api: bool = False):
    key = worker_key()
    authorization = request.headers.get("authorization", "")
    if model_api and not authorization and request.headers.get("x-api-key"):
        authorization = "Bearer " + request.headers["x-api-key"]
    if not hmac.compare_digest(authorization, "Bearer " + key):
        return JSONResponse({"error": "Bot authentication required"}, status_code=401)
    client = product_client()
    headers = {"Authorization": "Bearer " + key, "content-type": request.headers.get("content-type", "application/json")}
    if model_api:
        for name in ("anthropic-beta", "anthropic-version"):
            if request.headers.get(name): headers[name] = request.headers[name]
    try:
        upstream = await client.send(client.build_request(request.method, "http://product" + path, params=request.query_params, headers=headers, content=await request.body()), stream=True)
    except BaseException as error:
        if isinstance(error, httpx.HTTPError):
            return JSONResponse({"error": "Product service unavailable"}, status_code=502)
        raise
    async def chunks():
        try:
            async for chunk in upstream.aiter_bytes(): yield chunk
        finally: await upstream.aclose()
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
