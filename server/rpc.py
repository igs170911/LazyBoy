"""Native Hermes JSON-RPC client, including server-to-client human requests."""
import asyncio
import contextlib
import json
import logging

from websockets.asyncio.client import connect


class HermesRPC:
    def __init__(self, on_event, on_request, on_disconnect):
        self.on_event, self.on_request, self.on_disconnect = on_event, on_request, on_disconnect
        self.socket = None
        self.pending = {}
        self.counter = 0
        self.reader = None
        self.ready = asyncio.Event()
        self.alive = False

    async def open(self, port, key):
        self.socket = await connect(f"ws://127.0.0.1:{port}/rpc", additional_headers={"Authorization":"Bearer "+key}, max_size=16*1024*1024, ping_interval=15, ping_timeout=30)
        self.alive = True
        self.reader = asyncio.create_task(self._read())
        try:
            await asyncio.wait_for(self.ready.wait(), 15)
            await self.call("client.capabilities", {"server_requests": True})
        except BaseException:
            await self.close()
            raise

    async def call(self, method, params=None, timeout=90):
        if not self.alive: raise RuntimeError("Bot 連線已中斷")
        self.counter += 1
        id = str(self.counter)
        future = asyncio.get_running_loop().create_future()
        self.pending[id] = future
        try:
            await self.socket.send(json.dumps({"jsonrpc":"2.0","id":id,"method":method,"params":params or {}}, ensure_ascii=False))
            return await asyncio.wait_for(future, timeout)
        finally: self.pending.pop(id, None)

    async def answer(self, id, result):
        if not self.alive: raise RuntimeError("Bot 連線已中斷，請重新連線")
        await self.socket.send(json.dumps({"jsonrpc":"2.0","id":id,"result":result}, ensure_ascii=False))

    async def _dispatch(self, frame):
        if "method" in frame and "id" in frame:
            await self.on_request(frame)
        elif "id" in frame:
            future = self.pending.get(str(frame["id"]))
            if future and not future.done():
                if "error" in frame: future.set_exception(RuntimeError(frame["error"].get("message","Bot 操作失敗")))
                else: future.set_result(frame.get("result",{}))
        else:
            params = frame.get("params", {})
            if params.get("type") == "gateway.ready": self.ready.set()
            await self.on_event(params)

    async def _read(self):
        log = logging.getLogger(__name__)
        try:
            async for raw in self.socket:
                for line in raw.splitlines():
                    if not line.strip(): continue
                    # One malformed frame or failing handler must not tear down the transport.
                    try: await self._dispatch(json.loads(line))
                    except Exception: log.exception("Hermes frame handling failed")
        except Exception as exc:
            log.warning("Hermes event transport failed (%s)", type(exc).__name__)
        finally:
            was_alive = self.alive
            self.alive = False
            for future in self.pending.values():
                if not future.done(): future.set_exception(RuntimeError("Bot 連線已中斷"))
            with contextlib.suppress(Exception): await self.socket.close()
            if was_alive: await self.on_disconnect("Bot 連線已中斷")

    async def close(self):
        self.alive = False
        if self.socket: await self.socket.close()
        if self.reader and self.reader is not asyncio.current_task():
            self.reader.cancel()
            try: await self.reader
            except asyncio.CancelledError: pass
