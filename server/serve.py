"""A single product process listens on HTTP and the worker-only local socket."""
import asyncio
import contextlib
import faulthandler
import fcntl
import logging
import os
import signal
import socket
import uvicorn
from .config import DATA, HTTPS_PORT, PORT


def setup_logging():
    """The product's own warnings and errors, with time and source, next to uvicorn's lines in .run/server.log."""
    handler = logging.StreamHandler()
    handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s"))
    for name in ("server", "lazyhermes"):
        product = logging.getLogger(name)
        product.setLevel(logging.INFO); product.addHandler(handler); product.propagate = False


class Server(uvicorn.Server):
    """Ends the live event streams as soon as a stop is requested; an open tab would otherwise hold every restart
    for the full graceful-shutdown timeout and then be cancelled with a traceback."""
    partners = ()

    def handle_exit(self, sig, frame):
        from . import main as product
        with contextlib.suppress(Exception): product.begin_shutdown(self.loop)
        super().handle_exit(sig, frame)
        # Only the last server to start receives the signal; the HTTP and HTTPS servers stop together.
        for partner in self.partners:
            partner.should_exit = True
            partner.force_exit = partner.force_exit or self.force_exit

    async def serve(self, sockets=None):
        self.loop = asyncio.get_running_loop()
        await super().serve(sockets=sockets)


async def main():
    DATA.mkdir(parents=True, exist_ok=True)
    lock = (DATA / "service.lock").open("a+")
    try: fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError: raise SystemExit("這個資料目錄已有 LazyBoy 服務在執行")
    lock.seek(0); lock.truncate()
    lock.write(str(os.getpid())); lock.flush()
    directory = DATA / "ipc"; directory.mkdir(exist_ok=True, mode=0o700)
    path = directory / "product.sock"; path.unlink(missing_ok=True)
    local = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); local.bind(str(path)); os.chmod(path, 0o600)
    public = socket.socket(socket.AF_INET, socket.SOCK_STREAM); public.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    bind = os.environ.get("LAZYHERMES_BIND", "0.0.0.0")
    public.bind((bind, PORT))
    listeners = [local, public]
    # The desktop stream is already Tight/JPEG-compressed; deflating it again in the event loop costs ~15x relay time.
    options = dict(access_log=False, log_level="info", timeout_graceful_shutdown=5, ws_per_message_deflate=False)
    server = Server(uvicorn.Config("server.main:app", **options))
    servers = [server.serve(sockets=[public, local])]
    if HTTPS_PORT:
        from . import tls
        certificate, key = tls.ensure(DATA / "tls")
        secure = socket.socket(socket.AF_INET, socket.SOCK_STREAM); secure.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        secure.bind((bind, HTTPS_PORT)); listeners.append(secure)
        # Same app, already started by the HTTP server: no second lifespan.
        https = Server(uvicorn.Config("server.main:app", **options, lifespan="off", ssl_certfile=str(certificate), ssl_keyfile=str(key)))
        server.partners, https.partners = (https,), (server,)
        servers.append(https.serve(sockets=[secure]))
    for listener in listeners: listener.setblocking(False); listener.listen(2048)
    try: await asyncio.gather(*servers)
    finally:
        for listener in listeners: listener.close()
        path.unlink(missing_ok=True); lock.close()


if __name__ == "__main__":
    setup_logging()
    # `kill -USR1 <pid>` writes every thread's stack to the log, even when the event loop is stuck.
    faulthandler.enable()
    faulthandler.register(signal.SIGUSR1, all_threads=True)
    try: asyncio.run(main())
    except KeyboardInterrupt: pass
