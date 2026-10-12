"""Real Docker + unmodified Hermes loop, using a local deterministic model.

Run with `uv run python tests/native_continuation_flow.py`. No paid API or
existing Bot data is used. The fixture controls only its own UUID computers.
"""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import tempfile
import threading
import time

import httpx

ROOT=Path(__file__).resolve().parents[1]
MARKER="NATIVE_HERMES_CONTINUATION_VERIFIED"
SKILL_NAME="continuation-fixture"
SKILL_MARKER="NATIVE_SKILL_READ_BACK"
MEMORY_MARKER="NATIVE_PRIVATE_MEMORY_CONTINUATION"
requests=[]
failures=[]


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1",0))
        return s.getsockname()[1]


class Model(BaseHTTPRequestHandler):
    def log_message(self, *args): pass

    def do_GET(self):
        self.send_response(200); self.send_header("Content-Type","application/json"); self.end_headers()
        self.wfile.write(json.dumps({"data":[{"id":"fixture-model","object":"model"}]}).encode())

    def do_POST(self):
        try:
            self.respond()
        except Exception as exc:
            failures.append(str(exc))
            self.send_response(422); self.send_header("Content-Type","application/json"); self.end_headers()
            self.wfile.write(json.dumps({"error":str(exc)}).encode())

    def respond(self):
        body=json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        messages=body.get("messages",[])
        tools={t["function"]["name"]:t["function"] for t in body.get("tools",[])}
        last_user=max((i for i,m in enumerate(messages) if m.get("role")=="user"),default=-1)
        restarting=last_user>=0 and "RESTART_FIXTURE" in str(messages[last_user].get("content"))
        stopping=last_user>=0 and ("STOP_FIXTURE" in str(messages[last_user].get("content")) or restarting)
        questioning=last_user>=0 and "QUESTION_FIXTURE" in str(messages[last_user].get("content"))
        learning=last_user>=0 and "SKILL_FIXTURE" in str(messages[last_user].get("content"))
        switching=last_user>=0 and "MODEL_SWITCH_FIXTURE" in str(messages[last_user].get("content"))
        remembering=last_user>=0 and "MEMORY_WRITE_FIXTURE" in str(messages[last_user].get("content"))
        recalling=last_user>=0 and "MEMORY_RECALL_FIXTURE" in str(messages[last_user].get("content"))
        isolation=last_user>=0 and "MEMORY_ISOLATION_FIXTURE" in str(messages[last_user].get("content"))
        scheduling=last_user>=0 and "ROUTINE_FIXTURE" in str(messages[last_user].get("content"))
        steering=last_user>=0 and any(marker in str(messages[last_user].get("content")) for marker in ["STEER_FIXTURE","STEER_ADDED_FIXTURE"])
        steered=steering and "STEER_ADDED_FIXTURE" in str(messages[last_user].get("content"))
        excluded=last_user>=0 and any(marker in str(messages[last_user].get("content")) for marker in ["SKILL_DISABLED_FIXTURE","SKILL_REMOVED_FIXTURE"])
        requests.append({"tools":list(tools),"model":body.get("model"),"path":self.path,"authorization":self.headers.get("Authorization"),"last_role":messages[-1]["role"] if messages else None,
                         "last_content":messages[-1].get("content") if messages else None,"scenario":"restart" if restarting else "steer" if steering else "memory" if remembering or recalling or isolation else "routine" if scheduling else "model" if switching else "stop" if stopping else "question" if questioning else "skill" if learning or excluded else "primary" if tools else "aux"})
        previous=[m for m in messages[last_user+1:] if m.get("role")=="tool"]
        # The native loop must really write, then read the file. No narration is evidence.
        if not tools:
            # Hermes may also make a title/memory request outside the foreground turn.
            name=None; args={}
        elif stopping:
            name="terminal" if not previous else None
            command="printf started > /workspace/restart-started.txt; sleep 10; printf UNKNOWN_REPLAY > /workspace/native-after-restart.txt" if restarting else "printf started > /workspace/stop-started.txt; sleep 10; printf AFTER_STOP > /workspace/native-after-stop.txt"
            args={"command":command} if name else {}
        elif switching:
            name=None;args={}
        elif steering:
            if not steered:
                name="terminal" if not previous else None
                args={"command":"printf started > /workspace/steer-started.txt; sleep 2"} if name else {}
            elif not any("STEER_CONFIRMED" in str(m.get("tool_calls")) for m in messages[last_user+1:]):
                name="terminal";args={"command":"printf 'STEER_CONFIRMED,CSV\\n' > /workspace/steered-result.csv"}
            elif not any("STEER_CONFIRMED,CSV" in str(m.get("content")) for m in previous):
                name="read_file";args={"path":"/workspace/steered-result.csv"}
            else: name=None;args={}
        elif remembering:
            name="memory" if not previous else None;args={"action":"add","target":"memory","content":MEMORY_MARKER} if name else {}
        elif recalling or isolation:
            memory_prompt=str([m.get("content") for m in messages if m.get("role")=="system"])
            assert (MEMORY_MARKER in memory_prompt)==recalling,"Native memory was not loaded privately after conversation clear"
            name=None;args={}
        elif scheduling:
            if not previous:
                name="tool_search";args={"queries":["routines_manage"]}
            elif len(previous)==1:
                match=re.search(r"mcp_[\w]*routines_manage",str(previous[-1].get("content","")))
                assert match,"Native product scheduling tool was not discovered"
                name="tool_call";args={"calls":[{"name":match.group(),"arguments":{"action":"create","title":"Native fixture schedule","instructions":"MODEL_SWITCH_FIXTURE：執行例行驗證。","schedule":{"frequency":"once","date":"2030-01-01","time":"09:00","timezone":"Asia/Taipei"}}}]}
            else: name=None;args={}
        elif questioning:
            name="clarify" if not previous else None
            args={"questions":[{"question":"輸出格式？","choices":["CSV","JSON"]},{"question":"使用語言？","choices":["繁體中文","English"]}]} if name else {}
        elif learning:
            if not previous:
                name="skill_manage";args={"operations":[{"action":"create","name":SKILL_NAME,"content":f"---\nname: {SKILL_NAME}\ndescription: Use when verifying native continuation\n---\n\nRead back the produced artifact. {SKILL_MARKER}\n"}]}
            elif len(previous)==1: name="skill_view";args={"name":SKILL_NAME}
            else: name=None;args={}
        elif excluded:
            name="skills_list" if not previous else None;args={}
        elif not previous:
            name="terminal"
            args={"command":f"printf '%s\\n' '{MARKER}' > /workspace/native-proof.txt"}
        elif len(previous)==1:
            name="read_file"; args={"path":"/workspace/native-proof.txt"}
        elif len(previous)==2:
            name="browser_exec"
            args={"code":"new_tab('http://127.0.0.1:8642/health')\nprint(js('location.href'))\nprint(js('document.body.innerText'))\nprint(js(\"document.cookie = 'continuation_session=persisted; Max-Age=3600; Path=/'\"))"}
        elif len(previous)==3:
            name="tool_search";args={"queries":["desktop_observe"]}
        elif len(previous)==4:
            match=re.search(r"mcp_[\w]*desktop_observe",str(previous[-1].get("content","")))
            if not match: raise RuntimeError("Computer MCP observation tool was not discovered")
            name="tool_call";args={"calls":[{"name":match.group(),"arguments":{"image":False}}]}
        else: name=None; args={}
        if name and name not in tools: raise RuntimeError(f"Native tool unavailable: {name}; names={list(tools)}")
        if name:
            message={"role":"assistant","content":None,"tool_calls":[{"id":"fixture-call-"+str(len(requests)),"type":"function","function":{"name":name,"arguments":json.dumps(args)}}]}
        else:
            if tools and questioning:
                if not all(value in str(previous[-1].get("content")) for value in ["CSV","繁體中文"]): raise RuntimeError("Native clarification did not receive distinct answers")
            if tools and learning and SKILL_MARKER not in str(previous[-1].get("content")): raise RuntimeError("Native saved skill could not be read")
            if tools and scheduling and "Native fixture schedule" not in str(previous[-1].get("content")): raise RuntimeError("Native schedule was not created: "+str(previous[-1].get("content")))
            if tools and steering and not steered: raise RuntimeError("Native foreground never received steering")
            if tools and excluded and SKILL_NAME in str(previous[-1].get("content")): raise RuntimeError("Disabled/deleted skill remains in native inventory")
            if tools and not stopping and not questioning and not learning and not excluded and not switching and not remembering and not recalling and not isolation and not scheduling and not steering:
                if MARKER not in str(previous[1].get("content")): raise RuntimeError("Native read-back did not return the marker")
                if "http://127.0.0.1:8642/health" not in str(previous[2].get("content")): raise RuntimeError("Native browser did not observe the fixture page: "+str(previous[2].get("content")))
                if not previous[-1].get("content") or "did not approve" in str(previous[-1].get("content")): raise RuntimeError("Native desktop observation did not execute")
            message={"role":"assistant","content":"STEER_FIXTURE_VERIFIED" if steering else "MEMORY_FIXTURE_VERIFIED" if remembering or recalling or isolation else "ROUTINE_FIXTURE_VERIFIED" if scheduling else "MODEL_SWITCH_VERIFIED" if switching else "STOP_FIXTURE_DONE" if stopping else "QUESTION_FIXTURE：已收到 CSV 與繁體中文" if questioning else "SKILL_FIXTURE_VERIFIED" if learning or excluded else "已建立並讀回驗證 /workspace/native-proof.txt："+MARKER if tools else "{}"}
        self.send_response(200)
        if body.get("stream"):
            self.send_header("Content-Type","text/event-stream"); self.end_headers()
            delta={k:v for k,v in message.items() if k!="role"}
            if "tool_calls" in delta:
                delta["tool_calls"]=[{"index":i,**t} for i,t in enumerate(delta["tool_calls"])]
            payload={"id":"fixture-response","object":"chat.completion.chunk","created":int(time.time()),"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":None}]}
            self.wfile.write(("data: "+json.dumps(payload)+"\n\n").encode())
            payload["choices"]=[{"index":0,"delta":{},"finish_reason":"tool_calls" if name else "stop"}]
            self.wfile.write(("data: "+json.dumps(payload)+"\n\ndata: [DONE]\n\n").encode())
        else:
            self.send_header("Content-Type","application/json"); self.end_headers()
            self.wfile.write(json.dumps({"id":"fixture-response","object":"chat.completion","created":int(time.time()),"model":"fixture-model","choices":[{"index":0,"message":message,"finish_reason":"tool_calls" if name else "stop"}],"usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}}).encode())


def main():
    (ROOT/".run").mkdir(exist_ok=True)
    directory=Path(tempfile.mkdtemp(prefix="native-continuation-",dir=ROOT/".run"))
    port=free_port(); model=ThreadingHTTPServer(("127.0.0.1",0),Model)
    threading.Thread(target=model.serve_forever,daemon=True).start()
    image=subprocess.check_output(["docker","image","inspect",os.environ.get("NATIVE_FIXTURE_IMAGE","lazyhermes-bot:local"),"--format","{{.Id}}"],text=True).strip()
    fixture_image="lazyhermes-continuation:"+directory.name
    subprocess.run(["docker","tag",image,fixture_image],check=True)
    env={**os.environ,"LAZYHERMES_DATA_DIR":str(directory/"data"),"LAZYHERMES_PORT":str(port),"LAZYHERMES_BIND":"127.0.0.1","LAZYHERMES_IMAGE":fixture_image}
    env.pop("LAZYHERMES_ACCESS_TOKEN",None)
    log=(directory/"server.log").open("w")
    process=subprocess.Popen([str(ROOT/".venv/bin/python"),"-m","server.serve"],cwd=ROOT,env=env,stdout=log,stderr=log)
    agents=[]
    try:
        with httpx.Client(base_url=f"http://127.0.0.1:{port}",timeout=120) as client:
            deadline=time.monotonic()+20
            while True:
                if process.poll() is not None: raise RuntimeError("Disposable server exited; inspect "+str(directory/"server.log"))
                try:
                    if client.get("/api/health").is_success: break
                except httpx.HTTPError: pass
                if time.monotonic()>deadline: raise RuntimeError("Disposable server startup timed out")
                time.sleep(.2)
            agents=[a["id"] for a in client.get("/api/agents").json()["agents"]]
            duplicate=subprocess.run([str(ROOT/".venv/bin/python"),"-m","server.serve"],cwd=ROOT,env=env,capture_output=True,text=True,timeout=10)
            assert duplicate.returncode!=0 and "已有" in duplicate.stderr
            assert (directory/"data/service.lock").read_text()==str(process.pid),"Duplicate start destroyed the active service PID"
            controlled=subprocess.run([str(ROOT/".venv/bin/python"),str(ROOT/"scripts/control.py"),"wait"],cwd=ROOT,env=env,capture_output=True,text=True,timeout=20)
            assert controlled.returncode==0,controlled.stderr
            agent=agents[0]
            response=client.put("/api/settings",json={"active":"fixture","model":"fixture-model","provider":{"id":"fixture","base_url":f"http://127.0.0.1:{model.server_port}/v1","api_key":"fixture-key","models":[{"id":"fixture-model","protocol":"chat_completions"}]}})
            response.raise_for_status()
            message={"text":"請建立 /workspace/native-proof.txt 並讀回驗證，再操作這臺電腦的瀏覽器並觀察桌面。","request_id":"native-proof"}
            response=client.post(f"/api/agents/{agent}/messages",json=message);response.raise_for_status()
            task_id=response.json()["queued"]
            replay=client.post(f"/api/agents/{agent}/messages",json=message);replay.raise_for_status()
            assert replay.json()["queued"]==task_id
            print("Native fixture admitted once; waiting for real Hermes tools",flush=True)
            deadline=time.monotonic()+180
            while True:
                if failures: raise RuntimeError(failures[-1])
                task=client.get("/api/tasks/"+task_id).json()
                if task["state"]=="terminal": break
                if time.monotonic()>deadline: raise RuntimeError("Native task timed out; see "+str(directory/"server.log"))
                time.sleep(.5)
            assert task["verdict"]=="answer",task.get("result")
            proof=directory/"data/bots"/agent/"workspace/native-proof.txt"
            assert proof.read_text().strip()==MARKER
            assert len([r for r in requests if r["scenario"]=="primary"])==6,requests
            downloaded=client.get(f"/api/agents/{agent}/files/content",params={"path":"native-proof.txt"});downloaded.raise_for_status()
            assert downloaded.text.strip()==MARKER
            transcript=client.get(f"/api/agents/{agent}").json()["transcript"]
            assert len([m for m in transcript if m["role"]=="user"])==1
            assert not transcript[-1].get("streaming")
            computer=client.get(f"/api/agents/{agent}/computer").json()
            assert computer["ready"]
            viewer=client.get(computer["viewer_url"]);viewer.raise_for_status()
            assert "noVNC" in viewer.text
            artifact=next(a for a in task.get("artifacts",[]) if a["rel_path"]=="native-proof.txt")
            assert artifact["ok_on_disk"]
            # Read the shared Chromium independently: automation must have used this seat.
            browser_state=subprocess.check_output(["docker","exec","lazyhermes-bot-"+agent,"/opt/hermes/.venv/bin/python","-c","import httpx,json; print(json.dumps(httpx.get('http://127.0.0.1:9222/json').json()))"],text=True)
            assert "http://127.0.0.1:8642/health" in browser_state
            assert proof.stat().st_uid==os.getuid(),"Native artifact is not owned by the host user"

            response=client.put("/api/settings",json={"active":"alternate","model":"alternate-model","provider":{"id":"alternate","base_url":f"http://127.0.0.1:{model.server_port}/alternate/v1","api_key":"alternate-fixture-key","models":[{"id":"alternate-model","protocol":"chat_completions"}]}});response.raise_for_status()
            response=client.post(f"/api/agents/{agent}/messages",json={"text":"MODEL_SWITCH_FIXTURE：驗證下一回合使用新供應商。","request_id":"model-switch"});response.raise_for_status()
            model_task=response.json()["queued"];deadline=time.monotonic()+60
            while True:
                if failures: raise RuntimeError(failures[-1])
                state=client.get("/api/tasks/"+model_task).json()
                if state["state"]=="terminal": break
                if time.monotonic()>deadline: raise RuntimeError("Native model switch timed out")
                time.sleep(.1)
            assert state["verdict"]=="answer",state
            switched=next(r for r in requests if r["scenario"]=="model")
            assert switched["model"]=="alternate-model" and switched["path"]=="/alternate/v1/chat/completions",switched
            assert switched["authorization"]=="Bearer alternate-fixture-key",switched
            assert state["provider"]=="alternate" and state["model"]=="alternate-model"
            response=client.put("/api/settings",json={"active":"fixture","model":"fixture-model"});response.raise_for_status()

            # Native server-to-client clarify reaches the product and returns keyed answers.
            response=client.post(f"/api/agents/{agent}/messages",json={"text":"QUESTION_FIXTURE：請問我輸出格式和語言。","request_id":"question-fixture"});response.raise_for_status()
            question_task=response.json()["queued"]
            deadline=time.monotonic()+30
            while True:
                activity=client.get(f"/api/agents/{agent}/activity").json()
                question=activity.get("question")
                if question: break
                if time.monotonic()>deadline: raise RuntimeError("Native clarification never reached web API")
                time.sleep(.1)
            assert activity["state"]=="waiting_input"
            assert len(question["hermes_questions"])==2
            answers={q["qid"]:value for q,value in zip(question["hermes_questions"],["CSV","繁體中文"])}
            response=client.post(f"/api/agents/{agent}/questions",json={"question_id":question["question_id"],"answers":answers});response.raise_for_status()
            while True:
                state=client.get("/api/tasks/"+question_task).json()
                if state["state"]=="terminal": break
                if time.monotonic()>deadline: raise RuntimeError("Native clarification did not resume")
                time.sleep(.1)
            assert state["verdict"]=="answer"
            assert "CSV" in state["result"]["summary"] and "繁體中文" in state["result"]["summary"]

            def run_turn(text,key,expected="SKILL_FIXTURE_VERIFIED",target=agent):
                response=client.post(f"/api/agents/{target}/messages",json={"text":text,"request_id":key});response.raise_for_status()
                tid=response.json()["queued"]
                limit=time.monotonic()+60
                while True:
                    if failures: raise RuntimeError(failures[-1])
                    state=client.get("/api/tasks/"+tid).json()
                    if state["state"]=="terminal": break
                    if time.monotonic()>limit: raise RuntimeError("Native skill verification timed out")
                    time.sleep(.1)
                assert state["verdict"]=="answer",state
                assert expected in state["result"]["summary"],state
                return state
            run_turn("SKILL_FIXTURE：請建立並讀回可分享技能。","learn-fixture")
            response=client.post(f"/api/agents/{agent}/skills",json={"action":"list"});response.raise_for_status()
            learned=next(s for s in response.json()["skills"] if s["name"]==SKILL_NAME)
            assert SKILL_MARKER in learned["instructions"]
            template=client.get(f"/api/agents/{agent}/template");template.raise_for_status()
            assert any(s["name"]==SKILL_NAME for s in template.json()["skills"])
            run_turn("MEMORY_WRITE_FIXTURE：請記住驗收識別碼。","memory-write","MEMORY_FIXTURE_VERIFIED")
            memory=directory/"data/bots"/agent/"hermes/memories/MEMORY.md"
            assert MEMORY_MARKER in memory.read_text()
            response=client.delete(f"/api/agents/{agent}/conversation");response.raise_for_status()
            assert client.get(f"/api/agents/{agent}").json()["transcript"]==[]
            assert any(s["name"]==SKILL_NAME for s in client.post(f"/api/agents/{agent}/skills",json={"action":"list"}).json()["skills"])
            run_turn("MEMORY_RECALL_FIXTURE：讀取你的私有記憶。","memory-recall","MEMORY_FIXTURE_VERIFIED")
            response=client.post(f"/api/agents/{agent}/skills",json={"action":"toggle","id":learned["id"],"enabled":False});response.raise_for_status()
            run_turn("SKILL_DISABLED_FIXTURE：檢查可用技能列表。","disabled-fixture")
            response=client.post(f"/api/agents/{agent}/skills",json={"action":"delete","id":learned["id"]});response.raise_for_status()
            run_turn("SKILL_REMOVED_FIXTURE：檢查可用技能列表。","removed-fixture")
            import sqlite3
            conn=sqlite3.connect(directory/"data/product.sqlite3")
            notices=[json.loads(r[0]) for r in conn.execute("SELECT payload FROM events WHERE agent=? AND kind='skills_changed'",(agent,))]
            conn.close()
            assert any(n.get("saved") is True for n in notices),"Native learning never notified web UI"

            run_turn("ROUTINE_FIXTURE：建立一次性例行工作。","native-routine","ROUTINE_FIXTURE_VERIFIED")
            routines=client.get(f"/api/agents/{agent}/routines").json()["routines"]
            routine=next(r for r in routines if r["title"]=="Native fixture schedule")
            # Make this disposable occurrence overdue: the real product scheduler catches up once.
            conn=sqlite3.connect(directory/"data/product.sqlite3")
            routine["next_run_at"]="2020-01-01T01:00:00+00:00"
            conn.execute("UPDATE objects SET data=? WHERE bucket='routines' AND id=?",(json.dumps(routine),routine["id"]));conn.commit();conn.close()
            deadline=time.monotonic()+30
            while True:
                current=next(r for r in client.get(f"/api/agents/{agent}/routines").json()["routines"] if r["id"]==routine["id"])
                if current.get("last_task_id"):
                    scheduled=client.get("/api/tasks/"+current["last_task_id"]).json()
                    if scheduled["state"]=="terminal": break
                if time.monotonic()>deadline: raise RuntimeError("Native-created routine did not execute")
                time.sleep(.1)
            assert scheduled["verdict"]=="answer"
            time.sleep(2.2)
            history=client.post(f"/api/agents/{agent}/routines",json={"action":"history","id":routine["id"]}).json()["runs"]
            assert len(history)==1 and not current["enabled"]

            response=client.post(f"/api/agents/{agent}/messages",json={"text":"STEER_FIXTURE：開始等待補充。","request_id":"steer-original"});response.raise_for_status()
            steer_task=response.json()["queued"];deadline=time.monotonic()+30
            while not (proof.parent/"steer-started.txt").exists():
                if time.monotonic()>deadline: raise RuntimeError("Steering command never started")
                time.sleep(.1)
            supplement={"text":"STEER_ADDED_FIXTURE：請改用 CSV。","request_id":"steer-supplement"}
            response=client.post(f"/api/agents/{agent}/messages",json=supplement);response.raise_for_status()
            assert response.json().get("steered")==steer_task,response.json()
            replay=client.post(f"/api/agents/{agent}/messages",json=supplement);replay.raise_for_status()
            assert replay.json().get("queued",replay.json().get("steered"))==steer_task
            while True:
                if failures: raise RuntimeError(failures[-1])
                state=client.get("/api/tasks/"+steer_task).json()
                if state["state"]=="terminal": break
                if time.monotonic()>deadline: raise RuntimeError("Native steering did not finish")
                time.sleep(.1)
            assert state["verdict"]=="answer" and "STEER_FIXTURE_VERIFIED" in state["result"]["summary"],state
            assert (proof.parent/"steered-result.csv").read_text().strip()=="STEER_CONFIRMED,CSV"
            transcript=client.get(f"/api/agents/{agent}").json()["transcript"]
            assert len([m for m in transcript if "STEER_ADDED_FIXTURE" in m["content"]])==1

            # Interrupt a command that has demonstrably started, before its delayed write.
            response=client.post(f"/api/agents/{agent}/messages",json={"text":"STOP_FIXTURE：執行停止驗證指令。","request_id":"stop-fixture"});response.raise_for_status()
            stopped_task=response.json()["queued"]
            started=proof.parent/"stop-started.txt"
            deadline=time.monotonic()+60
            while not started.exists():
                state=client.get("/api/tasks/"+stopped_task).json()
                if state["state"]=="terminal": raise RuntimeError("Stop fixture ended before command started: "+str(state.get("result")))
                if time.monotonic()>deadline: raise RuntimeError("Stop fixture command never started")
                time.sleep(.1)
            result=client.post(f"/api/agents/{agent}/stop");result.raise_for_status()
            deadline=time.monotonic()+30
            while True:
                state=client.get("/api/tasks/"+stopped_task).json()
                if state["state"]=="terminal": break
                if time.monotonic()>deadline: raise RuntimeError("Native stop never completed")
                time.sleep(.1)
            assert state["verdict"]=="cancelled",state
            time.sleep(10.5)
            assert not (proof.parent/"native-after-stop.txt").exists(),"Cancelled tool continued writing"

            # A second real computer cannot access the first Bot's workspace.
            second=client.post("/api/agents",json={"name":"Isolated fixture"});second.raise_for_status()
            other=second.json()["id"];agents.append(other)
            response=client.post(f"/api/agents/{other}/computer",json={"action":"start"});response.raise_for_status()
            assert response.json()["ready"]
            assert client.get(f"/api/agents/{other}/files/content",params={"path":"native-proof.txt"}).status_code==404
            first_mounts=json.loads(subprocess.check_output(["docker","inspect","lazyhermes-bot-"+agent],text=True))[0]["Mounts"]
            second_mounts=json.loads(subprocess.check_output(["docker","inspect","lazyhermes-bot-"+other],text=True))[0]["Mounts"]
            for destination in ["/hermes","/workspace","/home/box"]:
                assert next(m["Source"] for m in first_mounts if m["Destination"]==destination)!=next(m["Source"] for m in second_mounts if m["Destination"]==destination)
            run_turn("MEMORY_ISOLATION_FIXTURE：檢查你沒有另一個 Bot 的記憶。","memory-isolated","MEMORY_FIXTURE_VERIFIED",target=other)
            def cookies(target):
                code="""import httpx,json
from websockets.sync.client import connect
url=httpx.get('http://127.0.0.1:9222/json/version').json()['webSocketDebuggerUrl']
with connect(url) as ws:
 ws.send(json.dumps({'id':1,'method':'Storage.getCookies'}))
 while True:
  reply=json.loads(ws.recv())
  if reply.get('id')==1:
   print(json.dumps(reply['result']['cookies']));break
"""
                deadline=time.monotonic()+20
                while True:
                    result=subprocess.run(["docker","exec","lazyhermes-bot-"+target,"/opt/hermes/.venv/bin/python","-c",code],capture_output=True,text=True)
                    if result.returncode==0: return json.loads(result.stdout)
                    if time.monotonic()>deadline: raise RuntimeError("CDP was unavailable after restart: "+result.stderr)
                    time.sleep(.2)
            assert any(c["name"]=="continuation_session" for c in cookies(agent))
            assert all(c["name"]!="continuation_session" for c in cookies(other)),"Different Bot reused browser login"
            response=client.post(f"/api/agents/{agent}/computer",json={"action":"restart"});response.raise_for_status()
            assert response.json()["ready"]
            assert proof.read_text().strip()==MARKER and MEMORY_MARKER in memory.read_text()
            assert any(c["name"]=="continuation_session" and c["value"]=="persisted" for c in cookies(agent)),"Browser login cookie was lost on restart"
            run_turn("MEMORY_RECALL_FIXTURE：重啟後讀取私有記憶。","memory-restart","MEMORY_FIXTURE_VERIFIED")
            response=client.post(f"/api/agents/{agent}/messages",json={"text":"RESTART_FIXTURE：啟動會被重啟中斷的指令。","request_id":"restart-active"});response.raise_for_status()
            interrupted_task=response.json()["queued"];deadline=time.monotonic()+30
            while not (proof.parent/"restart-started.txt").exists():
                if time.monotonic()>deadline: raise RuntimeError("Worker interruption fixture never started")
                time.sleep(.1)
            subprocess.run(["docker","restart","--time","1","lazyhermes-bot-"+agent],check=True,capture_output=True,timeout=30)
            while True:
                state=client.get("/api/tasks/"+interrupted_task).json()
                if state["state"]=="terminal": break
                if time.monotonic()>deadline: raise RuntimeError("Restarted worker left an active task indefinitely")
                time.sleep(.1)
            assert state["verdict"]=="interrupted",state
            run_turn("MODEL_SWITCH_FIXTURE：Worker 中斷後開始新的明確任務。","after-interruption","MODEL_SWITCH_VERIFIED")
            time.sleep(10.5)
            assert not (proof.parent/"native-after-restart.txt").exists(),"Unknown tool outcome was automatically replayed"
            report={"passed":True,"engine":"unmodified Hermes","image":image,"foreground_model_requests":len([r for r in requests if r["tools"]]),"total_model_requests":len(requests),"checks":["idempotent native admission","native terminal write","native read_file verification","native browser_exec navigation and DOM read","native MCP desktop observation","automation uses the desktop Chromium CDP","durable user/final transcript","downloaded artifact matches file","computer ready","noVNC HTTP viewer","native clarify reaches web API with two questions","distinct keyed answers resume native turn","native skill_manage and skill_view persist and read a workflow","learned skill appears in UI and export","disabled/deleted skill is excluded by the warm native executor","native learning emits saved UI notification","interrupt an actual running command","no write after confirmed cancellation","two Bot computers have independent home/workspace/Hermes mounts","second Bot cannot download first Bot file","native output owned by host UID","provider/model changes take effect on the next native turn","new provider receives its own credential and endpoint","native memory tool writes persistent private notes","conversation clear preserves skills/files and reloads native memory","other Bot native prompt excludes private memory","native MCP creates a routine visible in the web API","one overdue routine executes exactly once","native foreground receives steering and produces the requested CSV","retrying steering does not duplicate transcript","browser login cookies are isolated between Bots","browser login cookies survive computer restart","workspace and native memory survive computer restart","native turn resumes after computer restart","duplicate service start preserves the running PID","service control wait observes the isolated backend"],"fixture_directory":str(directory)}
            report["checks"] += ["forced worker restart records interrupted task","next explicit native task resumes durable history after interruption","unknown side-effecting tool is not automatically replayed"]
            (directory/"report.json").write_text(json.dumps(report,indent=2)+"\n")
            print(json.dumps(report),flush=True)
    finally:
        (directory/"model-evidence.json").write_text(json.dumps({"requests":requests,"failures":failures},ensure_ascii=False,indent=2)+"\n")
        process.terminate()
        try: process.wait(timeout=35)
        except subprocess.TimeoutExpired: process.kill();process.wait()
        for agent in agents:
            subprocess.run(["docker","rm","-f","lazyhermes-bot-"+agent],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        subprocess.run(["docker","image","rm",fixture_image],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        model.shutdown();model.server_close();log.close()


if __name__=="__main__": main()
