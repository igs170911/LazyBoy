"""A single product process listens on HTTP and the worker-only local socket."""
import asyncio
import fcntl
import os
import socket
import uvicorn
from .config import DATA, PORT


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
    public.bind((os.environ.get("LAZYHERMES_BIND", "0.0.0.0"), PORT))
    for listener in (local, public): listener.setblocking(False); listener.listen(2048)
    try: await uvicorn.Server(uvicorn.Config("server.main:app", access_log=False, log_level="info", timeout_graceful_shutdown=5,
                                              # The desktop stream is already Tight/JPEG-compressed; deflating it again in the event loop costs ~15x relay time.
                                              ws_per_message_deflate=False)).serve(sockets=[public, local])
    finally: public.close(); local.close(); path.unlink(missing_ok=True); lock.close()


if __name__ == "__main__":
    try: asyncio.run(main())
    except KeyboardInterrupt: pass
