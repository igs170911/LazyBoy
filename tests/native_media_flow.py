"""Verify native image/reasoning inputs and the real desktop MCP with no paid API.

Run: uv run python tests/native_media_flow.py
Uses its own product data, provider fixture and disposable UUID computer.
"""
import base64
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time

import httpx

ROOT = Path(__file__).resolve().parents[1]
wire = []
PNG = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aM6kAAAAASUVORK5CYII=")


class Model(BaseHTTPRequestHandler):
    def log_message(self, *args): pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if body.get("tools"):
            user = next(m for m in reversed(body["messages"]) if m["role"] == "user")
            parts = user.get("content", [])
            images = [part["image_url"]["url"] for part in parts if isinstance(part, dict) and part.get("type") == "image_url"]
            wire.append({"model":body["model"], "images":len(images), "image_is_png":bool(images) and base64.b64decode(images[0].partition(",")[2]).startswith(b"\x89PNG"), "reasoning_effort":body.get("reasoning_effort"), "reasoning":body.get("reasoning")})
        message = {"role":"assistant", "content":"MEDIA_INPUT_VERIFIED"}
        self.send_response(200)
        if body.get("stream"):
            self.send_header("Content-Type", "text/event-stream"); self.end_headers()
            chunk = {"id":"fixture", "object":"chat.completion.chunk", "model":body["model"], "choices":[{"index":0,"delta":message,"finish_reason":None}]}
            self.wfile.write(("data: " + json.dumps(chunk) + "\n\n").encode())
            chunk["choices"][0].update(delta={}, finish_reason="stop")
            self.wfile.write(("data: " + json.dumps(chunk) + "\n\ndata: [DONE]\n\n").encode())
        else:
            self.send_header("Content-Type", "application/json"); self.end_headers()
            self.wfile.write(json.dumps({"id":"fixture","object":"chat.completion","model":body["model"],"choices":[{"index":0,"message":message,"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}).encode())


DESKTOP_PROBE = r'''
import asyncio,base64,json,os,re
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client
import httpx

async def main():
    params=StdioServerParameters(command="/opt/hermes/.venv/bin/python",args=["/opt/lazyhermes/desktop_mcp.py"],env=dict(os.environ))
    async with stdio_client(params) as streams:
        async with ClientSession(*streams) as session:
            await session.initialize()
            invalid=await session.call_tool("desktop_act",{"action":"click","element":0})
            assert invalid.is_error,"Unobserved element was clicked"
            observed=await session.call_tool("desktop_observe",{"image":True})
            assert not observed.is_error,observed
            images=[c for c in observed.content if c.type=="image"]
            assert images and base64.b64decode(images[0].data).startswith(b"\x89PNG"),"Screenshot is not native MCP image content"
            text="\n".join(c.text for c in observed.content if c.type=="text")
            # SDK content conversion may wrap a string-list as JSON.
            if text.startswith("["):
                try:
                    value=json.loads(text)
                    if isinstance(value,list):text="\n".join(v for v in value if isinstance(v,str))
                except ValueError:pass
            match=re.search(r'^\[(\d+)\].*(?:Address and search bar|Search or enter address)',text,re.M)
            assert match,"Chromium address field was absent from desktop observation: "+text
            clicked=await session.call_tool("desktop_act",{"action":"click","element":int(match.group(1))})
            assert not clicked.is_error,clicked
            for action in [{"action":"key","key":"ctrl+a"},{"action":"type","text":"http://127.0.0.1:8642/health?desktop-proof=1"},{"action":"key","key":"Return"}]:
                result=await session.call_tool("desktop_act",action);assert not result.is_error,result
            for _ in range(30):
                pages=httpx.get("http://127.0.0.1:9222/json").json()
                if any("desktop-proof=1" in p.get("url","") for p in pages):break
                await asyncio.sleep(.1)
            else:raise AssertionError("Observed-element desktop click/type did not navigate the shared Chromium")
            print(json.dumps({"native_png_image":True,"observed_element_click":True,"desktop_navigation_in_cdp":True}))
asyncio.run(main())
'''


def main():
    directory = Path(tempfile.mkdtemp(prefix="native-media-", dir=ROOT/".run"))
    with socket.socket() as socket_:
        socket_.bind(("127.0.0.1",0)); port=socket_.getsockname()[1]
    model=ThreadingHTTPServer(("127.0.0.1",0),Model)
    threading.Thread(target=model.serve_forever,daemon=True).start()
    image=subprocess.check_output(["docker","image","inspect",os.environ.get("NATIVE_FIXTURE_IMAGE","lazyhermes-bot:local"),"--format","{{.Id}}"],text=True).strip()
    tag="lazyhermes-media:"+directory.name
    subprocess.run(["docker","tag",image,tag],check=True)
    env={**os.environ,"LAZYHERMES_DATA_DIR":str(directory/"data"),"LAZYHERMES_PORT":str(port),"LAZYHERMES_BIND":"127.0.0.1","LAZYHERMES_IMAGE":tag}
    env.pop("LAZYHERMES_ACCESS_TOKEN",None)
    log=(directory/"server.log").open("w"); bots=[]
    process=subprocess.Popen([str(ROOT/".venv/bin/python"),"-m","server.serve"],cwd=ROOT,env=env,stdout=log,stderr=log)
    try:
        with httpx.Client(base_url=f"http://127.0.0.1:{port}",timeout=120) as client:
            deadline=time.monotonic()+20
            while True:
                if process.poll() is not None:raise RuntimeError("Media fixture server exited")
                try:
                    if client.get("/api/health").is_success:break
                except httpx.HTTPError:pass
                if time.monotonic()>deadline:raise RuntimeError("Media fixture startup timed out")
                time.sleep(.2)
            bots=[a["id"] for a in client.get("/api/agents").json()["agents"]]; agent=bots[0]
            provider={"id":"media-fixture","base_url":f"http://127.0.0.1:{model.server_port}/v1","api_key":"fixture-only","models":[{"id":"gpt-5-fixture","protocol":"chat_completions","vision":False,"reasoning_effort":"high"}]}
            client.put("/api/settings",json={"active":provider["id"],"model":"gpt-5-fixture","provider":provider}).raise_for_status()
            assert client.get("/api/settings").json()["providers"][-1]["selectable"]
            response=client.post(f"/api/agents/{agent}/messages",json={"text":"先確認工作環境。","request_id":"text-warmup"});response.raise_for_status()
            deadline=time.monotonic()+120
            while True:
                state=client.get("/api/tasks/"+response.json()["queued"]).json()
                if state["state"]=="terminal":break
                if time.monotonic()>deadline:raise RuntimeError("Native text warmup timed out")
                time.sleep(.2)
            assert state["verdict"]=="answer" and len(wire)==1 and wire[0]["images"]==0,wire
            provider["models"][0]["vision"]=True
            client.put("/api/settings",json={"provider":provider}).raise_for_status()
            uploaded=client.post(f"/api/agents/{agent}/uploads?name=media-proof.png",content=PNG);uploaded.raise_for_status()
            assert client.get("/api/media",params={"agent_id":agent}).json()["capabilities"]["images"]
            response=client.post(f"/api/agents/{agent}/messages",json={"text":"請讀取圖片 media-proof.png。","request_id":"media-proof"});response.raise_for_status()
            task=response.json()["queued"]; deadline=time.monotonic()+120
            while True:
                state=client.get("/api/tasks/"+task).json()
                if state["state"]=="terminal":break
                if time.monotonic()>deadline:raise RuntimeError("Native image turn timed out")
                time.sleep(.2)
            assert state["verdict"]=="answer",state
            assert state["reasoning_effort"]=="high"
            assert len(wire)==2 and wire[-1]["images"]==1 and wire[-1]["image_is_png"],wire
            assert wire[-1]["reasoning_effort"]=="high" or (wire[-1]["reasoning"] or {}).get("effort")=="high",wire
            print("Native image bytes and reasoning choice reached the model",flush=True)
            desktop=subprocess.run(["docker","exec","--user",f"{os.getuid()}:{os.getgid()}","lazyhermes-bot-"+agent,"/opt/hermes/.venv/bin/python","-c",DESKTOP_PROBE],capture_output=True,text=True,timeout=90)
            (directory/"desktop-probe.log").write_text(desktop.stderr)
            assert desktop.returncode==0,desktop.stderr[-3500:]
            probe={"agent":agent,"server":"readonly-probe","command":"/opt/hermes/.venv/bin/python","args":["/opt/lazyhermes/desktop_mcp.py"]}
            response=client.post("/api/mcp",json={**probe,"action":"save"})
            assert response.is_success,response.text
            response=client.post("/api/mcp",json={"agent":agent,"server":"readonly-probe","action":"test"});response.raise_for_status()
            server=response.json()["servers"][0]
            assert server["serverStatus"]=="connected" and server["tools"],server
            client.post("/api/mcp",json={"agent":agent,"server":"readonly-probe","action":"remove"}).raise_for_status()
            report={"passed":True,"image":image,"model_inputs":wire,"vision_change_takes_effect_on_warm_worker":True,"desktop":json.loads(desktop.stdout),"external_mcp":True,"fixture_directory":str(directory)}
            (directory/"report.json").write_text(json.dumps(report,indent=2)+"\n"); print(json.dumps(report),flush=True)
    finally:
        process.terminate()
        try:process.wait(timeout=35)
        except subprocess.TimeoutExpired:process.kill();process.wait()
        for agent in bots:subprocess.run(["docker","rm","-f","lazyhermes-bot-"+agent],capture_output=True)
        subprocess.run(["docker","image","rm",tag],capture_output=True)
        model.shutdown(); model.server_close(); log.close()


if __name__=="__main__": main()
