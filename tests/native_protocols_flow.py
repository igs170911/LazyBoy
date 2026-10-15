"""Run real Hermes tools through Responses and Messages using local providers.

This checks the native SDK/transport, not just the product proxy headers.
Run: uv run python tests/native_protocols_flow.py
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
from native_media_flow import PNG, ROOT

requests = []
auxiliary_requests = []
failures = []


class Model(BaseHTTPRequestHandler):
    def log_message(self, *args): pass

    def do_POST(self):
        try: self.respond()
        except Exception as exc:
            failures.append(str(exc)); self.send_response(422)
            self.send_header("Content-Type","application/json"); self.end_headers()
            self.wfile.write(json.dumps({"error":str(exc)}).encode())

    def send_events(self, events):
        self.send_header("Content-Type","text/event-stream"); self.end_headers()
        for event in events:
            self.wfile.write(("event: "+event["type"]+"\ndata: "+json.dumps(event)+"\n\n").encode())
        self.wfile.flush()

    def respond(self):
        body=json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        protocol="responses" if self.path.endswith("/responses") else "messages" if self.path.endswith("/messages") else None
        assert protocol,"Unexpected native endpoint: "+self.path
        auxiliary=not body.get("tools")
        marker=protocol.upper()+"_FIXTURE"
        messages=body.get("input",[]) if protocol=="responses" else body.get("messages",[])
        assert isinstance(messages,list),"Native input did not use the protocol's message schema"
        start=max((i for i,m in enumerate(messages) if m.get("role")=="user" and marker in str(m.get("content"))),default=-1)
        assert auxiliary or start>=0,"Fresh protocol prompt missing"
        if auxiliary:
            previous=[]; has_image=False
        elif protocol=="responses":
            previous=[m for m in messages[start+1:] if m.get("type")=="function_call_output"]
            has_image=any(p.get("type")=="input_image" for p in messages[start].get("content",[]) if isinstance(p,dict))
        else:
            previous=[p for m in messages[start+1:] for p in m.get("content",[]) if isinstance(p,dict) and p.get("type")=="tool_result"]
            has_image=any(p.get("type")=="image" for p in messages[start].get("content",[]) if isinstance(p,dict))
        assert auxiliary or has_image,"Native image input was missing for "+protocol
        assert len(previous)<=2,"Native loop repeated a completed operation"
        names=[t.get("name") for t in body.get("tools",[])]
        assert auxiliary or ("terminal" in names and "read_file" in names),"Native tools were absent"
        (auxiliary_requests if auxiliary else requests).append({"protocol":protocol,"model":body["model"],"path":self.path,"image":has_image,"tool_results":len(previous),"authorization":self.headers.get("Authorization"),"x_api_key":self.headers.get("x-api-key")})
        name=None if auxiliary else "terminal" if not previous else "read_file" if len(previous)==1 else None
        path="/workspace/"+protocol+"-proof.txt"; proof="Protocol fixture session" if auxiliary else "NATIVE_"+protocol.upper()+"_VERIFIED"
        args={"command":"printf '"+proof+"' > "+path} if name=="terminal" else {"path":path} if name else {}
        if not name and not auxiliary: assert proof in str(previous[-1]),"Actual native read-back was missing"
        self.send_response(200)
        if protocol=="responses":
            item={"type":"function_call","id":"fc"+str(len(previous)),"call_id":"call"+str(len(previous)),"name":name,"arguments":json.dumps(args),"status":"completed"} if name else {"type":"message","id":"msg-final","role":"assistant","status":"completed","content":[{"type":"output_text","text":proof,"annotations":[]}]}
            response={"id":"resp-"+str(len(requests)),"object":"response","created_at":int(time.time()),"status":"completed","model":body["model"],"output":[item],"usage":{"input_tokens":10,"output_tokens":10,"total_tokens":20}}
            if body.get("stream"):
                events=[{"type":"response.created","response":{**response,"status":"in_progress","output":[]}}, {"type":"response.output_item.added","output_index":0,"item":{**item,"status":"in_progress"}}]
                if name:
                    events.extend([{"type":"response.function_call_arguments.delta","item_id":item["id"],"output_index":0,"delta":json.dumps(args)}, {"type":"response.function_call_arguments.done","item_id":item["id"],"output_index":0,"arguments":json.dumps(args)}])
                else:
                    events.append({"type":"response.output_text.delta","item_id":item["id"],"output_index":0,"content_index":0,"delta":proof})
                events.extend([{"type":"response.output_item.done","output_index":0,"item":item},{"type":"response.completed","response":response}])
                self.send_events(events)
            else:
                self.send_header("Content-Type","application/json");self.end_headers();self.wfile.write(json.dumps(response).encode())
        else:
            block={"type":"tool_use","id":"call"+str(len(previous)),"name":name,"input":args} if name else {"type":"text","text":proof}
            response={"id":"msg-"+str(len(requests)),"type":"message","role":"assistant","model":body["model"],"content":[block],"stop_reason":"tool_use" if name else "end_turn","stop_sequence":None,"usage":{"input_tokens":10,"output_tokens":10}}
            if body.get("stream"):
                self.send_events([{"type":"message_start","message":{**response,"content":[],"stop_reason":None}}, {"type":"content_block_start","index":0,"content_block":{**block,**({"input":{}} if name else {"text":""})}}, {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":json.dumps(args)} if name else {"type":"text_delta","text":proof}}, {"type":"content_block_stop","index":0}, {"type":"message_delta","delta":{"stop_reason":response["stop_reason"],"stop_sequence":None},"usage":{"output_tokens":10}}, {"type":"message_stop"}])
            else:
                self.send_header("Content-Type","application/json");self.end_headers();self.wfile.write(json.dumps(response).encode())


def main():
    directory=Path(tempfile.mkdtemp(prefix="native-protocols-",dir=ROOT/".run"))
    with socket.socket() as s:s.bind(("127.0.0.1",0));port=s.getsockname()[1]
    model=ThreadingHTTPServer(("127.0.0.1",0),Model)
    threading.Thread(target=model.serve_forever,daemon=True).start()
    image=subprocess.check_output(["docker","image","inspect",os.environ.get("NATIVE_FIXTURE_IMAGE","lazyhermes-bot:local"),"--format","{{.Id}}"],text=True).strip()
    tag="lazyhermes-protocols:"+directory.name;subprocess.run(["docker","tag",image,tag],check=True)
    env={**os.environ,"LAZYHERMES_DATA_DIR":str(directory/"data"),"LAZYHERMES_PORT":str(port),"LAZYHERMES_BIND":"127.0.0.1","LAZYHERMES_IMAGE":tag};env.pop("LAZYHERMES_ACCESS_TOKEN",None)
    log=(directory/"server.log").open("w");bots=[]
    process=subprocess.Popen([str(ROOT/".venv/bin/python"),"-m","server.serve"],cwd=ROOT,env=env,stdout=log,stderr=log)
    try:
        with httpx.Client(base_url=f"http://127.0.0.1:{port}",timeout=120) as client:
            deadline=time.monotonic()+20
            while True:
                if process.poll() is not None:raise RuntimeError("Protocol server exited")
                try:
                    if client.get("/api/health").is_success:break
                except httpx.HTTPError:pass
                if time.monotonic()>deadline:raise RuntimeError("Protocol fixture startup timed out")
                time.sleep(.2)
            bots=[a["id"] for a in client.get("/api/agents").json()["agents"]]; agent=bots[0]
            client.post(f"/api/agents/{agent}/uploads?name=protocol-image.png",content=PNG).raise_for_status()
            for protocol in ["responses","messages"]:
                model_id="gpt-5-fixture" if protocol=="responses" else "claude-fixture"
                configuration={"product_url":f"http://127.0.0.1:{port}","label":protocol+" fixture","base_url":f"http://127.0.0.1:{model.server_port}/v1","api_key":"fixture-"+protocol,"model":model_id,"protocol":protocol}
                configured=subprocess.run(["node",str(ROOT/"web/e2e/hermes/configure-provider.mjs"),json.dumps(configuration)],cwd=ROOT,capture_output=True,text=True,timeout=60)
                assert configured.returncode==0,configured.stderr
                assert json.loads(configured.stdout)["protocol"]==protocol
                response=client.post(f"/api/agents/{agent}/messages",json={"text":protocol.upper()+"_FIXTURE：使用 protocol-image.png，實際寫檔並讀回。","request_id":protocol});response.raise_for_status()
                deadline=time.monotonic()+120
                while True:
                    if failures:raise RuntimeError(failures[-1])
                    state=client.get("/api/tasks/"+response.json()["queued"]).json()
                    if state["state"]=="terminal":break
                    if time.monotonic()>deadline:raise RuntimeError("Native "+protocol+" turn timed out")
                    time.sleep(.2)
                assert state["verdict"]=="answer",state
                content=client.get(f"/api/agents/{agent}/files/content",params={"path":protocol+"-proof.txt"});content.raise_for_status()
                assert content.text=="NATIVE_"+protocol.upper()+"_VERIFIED"
                rows=[r for r in requests if r["protocol"]==protocol]
                assert len(rows)==3,rows
                assert all(r["authorization"]=="Bearer fixture-"+protocol for r in rows),rows
                if protocol=="messages":assert all(r["x_api_key"]=="fixture-messages" for r in rows),rows
                print("Native "+protocol+" wrote/read the artifact and received the image",flush=True)
            report={"passed":True,"image":image,"requests":requests,"auxiliary_requests":len(auxiliary_requests),"checks":["web adds provider/manual model/API protocol and persists settings","Responses native tool loop and image input","Messages native tool loop and image input","warm worker switches protocol/provider/model","actual downloaded artifact verifies each transport"],"fixture_directory":str(directory)}
            (directory/"report.json").write_text(json.dumps(report,indent=2)+"\n");print(json.dumps(report),flush=True)
    finally:
        (directory/"requests.json").write_text(json.dumps({"requests":requests,"auxiliary_requests":auxiliary_requests,"failures":failures},indent=2)+"\n")
        process.terminate()
        try:process.wait(timeout=35)
        except subprocess.TimeoutExpired:process.kill();process.wait()
        for agent in bots:subprocess.run(["docker","rm","-f","lazyhermes-bot-"+agent],capture_output=True)
        subprocess.run(["docker","image","rm",tag],capture_output=True)
        model.shutdown();model.server_close();log.close()


if __name__=="__main__":main()
