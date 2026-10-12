"""Actual Hermes tools across chat/work promotion and pre-stream fallback.

Only disposable product data, UUID computers and a local deterministic model.
"""
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

ROOT=Path(__file__).resolve().parents[1]
wire=[]


class Model(BaseHTTPRequestHandler):
    def log_message(self,*args): pass
    def do_POST(self):
        body=json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        messages=body.get("messages",[])
        user=max((i for i,m in enumerate(messages) if m.get("role")=="user"),default=-1)
        scenario="fallback" if user>=0 and "FALLBACK_FIXTURE" in str(messages[user].get("content")) else "promotion"
        tools=body.get("tools",[])
        results=[m for m in messages[user+1:] if m.get("role")=="tool"]
        if tools:
            wire.append({"scenario":scenario,"model":body["model"],"authorization":self.headers.get("Authorization"),"tool_results":len(results)})
            if scenario=="fallback" and body["model"]=="fast":
                self.send_response(429);self.send_header("Content-Type","application/json");self.end_headers()
                self.wfile.write(b'{"error":"fixture rate limit"}');return
        if not tools: message={"role":"assistant","content":"{}"}
        elif len(results)<2:
            name="terminal" if not results else "read_file"
            arguments={"command":f"printf 'NATIVE_ROUTING_{scenario.upper()}' > /workspace/{scenario}.txt"} if not results else {"path":f"/workspace/{scenario}.txt"}
            message={"role":"assistant","content":None,"tool_calls":[{"id":f"routing-{scenario}-{len(wire)}","type":"function","function":{"name":name,"arguments":json.dumps(arguments)}}]}
        else:
            assert "NATIVE_ROUTING_" in str(results[-1]["content"])
            message={"role":"assistant","content":"Verified /workspace/"+scenario+".txt"}
        self.send_response(200)
        if body.get("stream"):
            self.send_header("Content-Type","text/event-stream");self.end_headers()
            delta={k:v for k,v in message.items() if k!="role"}
            if "tool_calls" in delta: delta["tool_calls"]=[{"index":i,**t} for i,t in enumerate(delta["tool_calls"])]
            chunk={"id":"routing-fixture","object":"chat.completion.chunk","model":body["model"],"choices":[{"index":0,"delta":delta,"finish_reason":None}]}
            self.wfile.write(("data: "+json.dumps(chunk)+"\n\n").encode())
            chunk["choices"]=[{"index":0,"delta":{},"finish_reason":"tool_calls" if message.get("tool_calls") else "stop"}]
            self.wfile.write(("data: "+json.dumps(chunk)+"\n\ndata: [DONE]\n\n").encode())
        else:
            self.send_header("Content-Type","application/json");self.end_headers()
            self.wfile.write(json.dumps({"id":"routing-fixture","object":"chat.completion","model":body["model"],"choices":[{"index":0,"message":message,"finish_reason":"tool_calls" if message.get("tool_calls") else "stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}).encode())


def main():
    (ROOT/".run").mkdir(exist_ok=True)
    directory=Path(tempfile.mkdtemp(prefix="native-routing-",dir=ROOT/".run"))
    with socket.socket() as s: s.bind(("127.0.0.1",0));port=s.getsockname()[1]
    model=ThreadingHTTPServer(("127.0.0.1",0),Model)
    threading.Thread(target=model.serve_forever,daemon=True).start()
    image=subprocess.check_output(["docker","image","inspect",os.environ.get("NATIVE_FIXTURE_IMAGE","lazyhermes-bot:local"),"--format","{{.Id}}"],text=True).strip()
    tag="lazyhermes-routing:"+directory.name
    subprocess.run(["docker","tag",image,tag],check=True)
    env={**os.environ,"LAZYHERMES_DATA_DIR":str(directory/"data"),"LAZYHERMES_PORT":str(port),"LAZYHERMES_BIND":"127.0.0.1","LAZYHERMES_IMAGE":tag}
    env.pop("LAZYHERMES_ACCESS_TOKEN",None)
    log=(directory/"server.log").open("w");bots=[]
    process=subprocess.Popen([str(ROOT/".venv/bin/python"),"-m","server.serve"],cwd=ROOT,env=env,stdout=log,stderr=log)
    try:
        with httpx.Client(base_url=f"http://127.0.0.1:{port}",timeout=120) as client:
            deadline=time.monotonic()+20
            while True:
                if process.poll() is not None: raise RuntimeError("Fixture server exited")
                try:
                    if client.get("/api/health").is_success: break
                except httpx.HTTPError: pass
                if time.monotonic()>deadline: raise RuntimeError("Fixture startup timed out")
                time.sleep(.2)
            bots=[b["id"] for b in client.get("/api/agents").json()["agents"]];agent=bots[0]
            provider={"id":"routing-fixture","base_url":f"http://127.0.0.1:{model.server_port}/v1","api_key":"fixture-only","models":[{"id":m,"protocol":"chat_completions"} for m in ("fast","work")]}
            target=lambda m:{"provider":provider["id"],"model":m}
            client.put("/api/settings",json={"active":provider["id"],"model":"work","provider":provider,"routing":{"chat":target("fast"),"work":target("work")}}).raise_for_status()
            for scenario in ("promotion","fallback"):
                response=client.post(f"/api/agents/{agent}/messages",json={"text":scenario.upper()+"_FIXTURE","request_id":scenario});response.raise_for_status()
                task_id=response.json()["queued"];deadline=time.monotonic()+180
                while True:
                    task=client.get("/api/tasks/"+task_id).json()
                    if task["state"]=="terminal": break
                    if time.monotonic()>deadline: raise RuntimeError("Native routing task timed out")
                    time.sleep(.2)
                assert task["verdict"]=="answer",task
                downloaded=client.get(f"/api/agents/{agent}/files/content",params={"path":scenario+".txt"});downloaded.raise_for_status()
                assert downloaded.text=="NATIVE_ROUTING_"+scenario.upper()
                requests=[r for r in wire if r["scenario"]==scenario]
                expected=["fast","work","work","work"] if scenario=="fallback" else ["fast","work","work"]
                assert [r["model"] for r in requests]==expected
                assert all(r["authorization"]=="Bearer fixture-only" for r in requests)
                assert task["routing"]["role"]=="work"
                print("Native "+scenario+" verified",flush=True)
            report={"passed":True,"image":image,"requests":wire,"checks":["actual native SDK starts on chat model","tool execution upgrades next native request","paired results reach work model","429 switches before any output","downloaded artifacts independently match"]}
            (directory/"report.json").write_text(json.dumps(report,indent=2)+"\n")
            print(json.dumps(report),flush=True)
    finally:
        process.terminate()
        try: process.wait(timeout=35)
        except subprocess.TimeoutExpired: process.kill();process.wait()
        for agent in bots: subprocess.run(["docker","rm","-f","lazyhermes-bot-"+agent],capture_output=True)
        subprocess.run(["docker","image","rm",tag],capture_output=True)
        model.shutdown();model.server_close();log.close()


if __name__=="__main__": main()
