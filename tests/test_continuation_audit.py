"""Cross-check the Hermes migration against LazyBoy2's user-facing contract.

All data is disposable. These tests never use a paid model or a real computer.
"""
import asyncio
import json
from datetime import datetime, timezone
from types import SimpleNamespace

import httpx
import pytest
import pytest_asyncio

from server.config import Settings
from server.connectors import Connectors
from server.runtime import Runtime
from server.scheduling import Scheduler, next_occurrence
from server.skills import Skills
from server.store import Store, uid


@pytest.mark.asyncio
async def test_worker_accepts_native_messages_auth_only_on_model_routes(tmp_path, monkeypatch):
    import importlib.util
    from pathlib import Path
    spec = importlib.util.spec_from_file_location("hermes_host_auth", Path(__file__).resolve().parents[1] / "box/hermes_host.py")
    host = importlib.util.module_from_spec(spec); spec.loader.exec_module(host)
    (tmp_path / ".worker-key").write_text("worker-fixture")
    monkeypatch.setenv("HERMES_HOME", str(tmp_path))
    seen = []
    async def upstream(request):
        seen.append(request)
        return httpx.Response(200, json={"ok":True})
    monkeypatch.setattr(host, "httpx", SimpleNamespace(
        AsyncClient=lambda **kwargs: httpx.AsyncClient(transport=httpx.MockTransport(upstream)),
        AsyncHTTPTransport=lambda **kwargs: None,
        Timeout=httpx.Timeout,
    ))
    async with httpx.AsyncClient(transport=httpx.ASGITransport(app=host.app), base_url="http://worker") as client:
        path = "/model/messages/v1/messages"
        assert (await client.post(path, headers={"x-api-key":"wrong"}, json={})).status_code == 401
        assert (await client.post("/product", headers={"x-api-key":"worker-fixture"}, json={})).status_code == 401
        response = await client.post(path, headers={"x-api-key":"worker-fixture", "anthropic-beta":"fixture-beta", "anthropic-version":"2023-06-01"}, json={"model":"fixture"})
        assert response.status_code == 200
        assert seen[-1].headers["authorization"] == "Bearer worker-fixture"
        assert "x-api-key" not in seen[-1].headers
        assert seen[-1].headers["anthropic-beta"] == "fixture-beta"
        assert seen[-1].headers["anthropic-version"] == "2023-06-01"
        assert json.loads(seen[-1].content) == {"model":"fixture"}
        assert (await client.post("/product", headers={"authorization":"Bearer worker-fixture"}, json={})).status_code == 200


def test_worker_boot_retires_only_its_previous_container_leases(tmp_path):
    import importlib.util
    from pathlib import Path
    import sqlite3
    spec=importlib.util.spec_from_file_location("hermes_host_fixture",Path(__file__).resolve().parents[1]/"box/hermes_host.py")
    host=importlib.util.module_from_spec(spec);spec.loader.exec_module(host)
    (tmp_path/".product-host-namespace").write_text("123\n")
    conn=sqlite3.connect(tmp_path/"state.db")
    for table in ["session_turn_leases","compression_locks"]:
        conn.execute(f"CREATE TABLE {table}(holder TEXT)")
        conn.executemany(f"INSERT INTO {table} VALUES(?)",[("pid=7:pidns=123:turn=old",),("pid=7:pidns=999:turn=foreign",),("pid=7:turn=unstamped",)])
    conn.commit()
    lock=host.prepare_host_state(tmp_path,namespace="456")
    try:
        assert json.loads((tmp_path/".product-host-namespace").read_text())["namespace"]=="456"
        for table in ["session_turn_leases","compression_locks"]:
            assert conn.execute(f"SELECT holder FROM {table}").fetchall()==[("pid=7:pidns=999:turn=foreign",),("pid=7:turn=unstamped",)]
        with pytest.raises(BlockingIOError): host.prepare_host_state(tmp_path,namespace="789")
        assert json.loads((tmp_path/".product-host-namespace").read_text())["namespace"]=="456"
    finally: lock.close();conn.close()


def test_worker_boot_handles_reused_pid_namespace_without_clearing_other_owners(tmp_path):
    import importlib.util
    from pathlib import Path
    import sqlite3
    spec=importlib.util.spec_from_file_location("hermes_host_same_namespace",Path(__file__).resolve().parents[1]/"box/hermes_host.py")
    host=importlib.util.module_from_spec(spec);spec.loader.exec_module(host)
    (tmp_path/".product-host-namespace").write_text(json.dumps({"namespace":"123","pid":7}))
    conn=sqlite3.connect(tmp_path/"state.db")
    conn.execute("CREATE TABLE session_turn_leases(holder TEXT)")
    conn.executemany("INSERT INTO session_turn_leases VALUES(?)",[("pid=7:pidns=123:turn=old-host",),("pid=99:pidns=123:turn=other-owner",)])
    conn.commit()
    lock=host.prepare_host_state(tmp_path,namespace="123")
    try: assert conn.execute("SELECT holder FROM session_turn_leases").fetchall()==[("pid=99:pidns=123:turn=other-owner",)]
    finally: lock.close();conn.close()


@pytest.fixture
def product(tmp_path):
    store = Store(tmp_path)
    settings = Settings(store)
    computers = SimpleNamespace(prepare=lambda id:store.agent_directory(id))
    runtime = Runtime(store, settings, computers)
    yield SimpleNamespace(store=store, settings=settings, runtime=runtime, computers=computers)
    store.conn.close()


def bot(product, name="Test Bot"):
    id=uid()
    product.store.put("agents", id, {"id":id, "name":name})
    product.store.agent_directory(id)
    return id


def active_task(product, agent):
    task=uid()
    product.store.put("tasks", task, {"id":task, "agent_id":agent, "goal":"original goal", "state":"running", "session_id":"session", "artifacts":[]})
    product.runtime.active[agent]=task
    return task


def test_nested_transaction_rolls_back_all_records(product):
    with pytest.raises(ValueError):
        with product.store.transaction():
            product.store.put("agents", "one", {"id":"one"})
            product.store.message("one", "user", "message")
            product.store.event("one", "user_message", {})
            raise ValueError("invalid remainder")
    assert product.store.all("agents")==[]
    assert product.store.transcript("one")==[]
    assert product.store.events("one")==[]


@pytest.mark.asyncio
async def test_duplicate_admission_survives_store_reopen(product):
    agent=bot(product)
    product.runtime.closed=True
    result=await product.runtime.submit(agent, "create a report", "stable-request")
    reopened=Store(product.store.directory)
    try:
        runtime=Runtime(reopened, product.settings, product.computers)
        runtime.closed=True
        replay=await runtime.submit(agent, "create a report", "stable-request")
        assert replay["queued"]==result["queued"]
        assert len(reopened.all("tasks"))==1
        assert len(reopened.transcript(agent))==1
        with pytest.raises(ValueError): await runtime.submit(agent, "different goal", "stable-request")
    finally: reopened.conn.close()


@pytest.mark.asyncio
async def test_simultaneous_duplicate_steering_reaches_worker_once(product):
    agent=bot(product)
    task=active_task(product, agent)
    calls=[]
    async def call(method, params):
        calls.append((method, params))
        await asyncio.sleep(.01)
        return {"status":"queued"}
    product.runtime.clients[agent]=SimpleNamespace(alive=True, call=call)
    results=await asyncio.gather(*(product.runtime.submit(agent, "use CSV instead", "same-steer") for _ in range(2)))
    assert len(calls)==1
    assert len(product.store.transcript(agent))==1
    assert all(r.get("steered", r.get("queued"))==task for r in results)


@pytest.mark.asyncio
async def test_stop_waits_for_worker_completion_and_cancels_queue(product):
    agent=bot(product)
    task=active_task(product, agent)
    queued=uid()
    product.store.put("tasks", queued, {"id":queued,"agent_id":agent,"state":"queued","goal":"next"})
    calls=[]
    async def call(method, params): calls.append(method); return {"status":"interrupted"}
    product.runtime.clients[agent]=SimpleNamespace(alive=True, call=call)
    result=await product.runtime.stop(agent)
    assert result["status"]=="stopping"
    assert product.runtime.task(task)["state"]=="stopping"
    assert product.runtime.task(queued)["verdict"]=="cancelled"
    assert calls==["session.interrupt"]
    await product.runtime.event(agent, {"type":"message.complete", "session_id":"session", "payload":{"status":"interrupted","text":"Stopped"}})
    assert product.runtime.task(task)["state"]=="terminal"
    assert product.runtime.task(task)["verdict"]=="cancelled"
    await asyncio.sleep(0)


@pytest.mark.asyncio
async def test_restart_marks_unknown_outcome_interrupted_without_replay(product):
    agent=bot(product)
    task=active_task(product, agent)
    product.runtime.active.clear()
    product.runtime.closed=True
    await product.runtime.startup()
    assert product.runtime.task(task)["verdict"]=="interrupted"
    assert product.store.transcript(agent)[-1]["role"]=="assistant"
    assert product.runtime.runners=={}


@pytest.mark.asyncio
async def test_second_session_call_uses_connection_local_cache(product):
    agent=bot(product)
    calls=[]
    async def call(method, params):
        calls.append(method)
        return {"session_id":"live-session","stored_session_id":"durable-session"}
    client=SimpleNamespace(call=call)
    assert await product.runtime.session(agent, client)=="live-session"
    assert await product.runtime.session(agent, client)=="live-session"
    assert calls==["session.create"]


@pytest.mark.asyncio
async def test_secret_answer_never_enters_product_history_or_events(product):
    agent=bot(product)
    task=active_task(product, agent)
    answers=[]
    async def answer(id, result): answers.append(result)
    product.runtime.clients[agent]=SimpleNamespace(alive=True, answer=answer)
    await product.runtime.human_request(agent, {"jsonrpc":"2.0","id":"srq-secret","method":"secret","params":{"prompt":"Password"}})
    await product.runtime.answer(agent, "srq-secret", value="PRIVATE_TEST_VALUE_01234")
    assert answers==[{"value":"PRIVATE_TEST_VALUE_01234"}]
    assert product.runtime.task(task)["state"]=="running"
    assert "PRIVATE_TEST_VALUE" not in json.dumps(product.store.transcript(agent)+product.store.events(agent))


@pytest.mark.asyncio
async def test_clarify_answer_uses_native_qid(product):
    agent=bot(product)
    active_task(product, agent)
    answers=[]
    async def answer(id, result): answers.append(result)
    product.runtime.clients[agent]=SimpleNamespace(alive=True, answer=answer)
    await product.runtime.human_request(agent, {"id":"srq-question","method":"clarify","params":{"questions":[{"qid":"format","question":"Output format?","choices":["CSV","JSON"]}]}})
    assert product.runtime.activity(agent)["question"]["options"]==["CSV","JSON"]
    await product.runtime.answer(agent, "srq-question", text="CSV")
    assert answers[0]["answers"]=={"format":"CSV"}


@pytest.mark.asyncio
async def test_free_text_does_not_grant_approval(product):
    agent=bot(product)
    active_task(product, agent)
    answers=[]
    async def answer(id, result): answers.append(result)
    product.runtime.clients[agent]=SimpleNamespace(alive=True, answer=answer)
    await product.runtime.human_request(agent, {"id":"srq-approval","method":"approval","params":{"command":"publish"}})
    await product.runtime.answer(agent, "srq-approval", text="先等等，請補充說明")
    assert answers[0]["choice"]=="deny"


@pytest.mark.asyncio
async def test_late_delta_cannot_reopen_completed_reply(product):
    agent=bot(product)
    task=active_task(product,agent)
    product.runtime.finish(agent,task,"answer","verified final")
    await product.runtime.event(agent,{"type":"message.delta","session_id":"session","payload":{"text":"late chunk"}})
    final=product.store.transcript(agent)[-1]
    assert final["content"]=="verified final"
    assert final["streaming"] is False


@pytest.mark.asyncio
async def test_normal_rpc_close_notifies_runtime_and_rejects_pending_calls():
    from websockets.asyncio.server import serve
    from server.rpc import HermesRPC
    entered=asyncio.Event()
    close=asyncio.Event()
    async def server(socket):
        await socket.send(json.dumps({"method":"event","params":{"type":"gateway.ready","payload":{}}}))
        first=json.loads(await socket.recv())
        assert first["method"]=="client.capabilities"
        await socket.send(json.dumps({"id":first["id"],"result":{"ok":True}}))
        pending=json.loads(await socket.recv())
        assert pending["method"]=="never.respond"
        entered.set()
        await close.wait()
        await socket.close(code=1000)
    events=[]
    disconnects=[]
    async def event(value): events.append(value)
    async def request(value): pass
    async def disconnected(value): disconnects.append(value)
    async with serve(server,"127.0.0.1",0) as host:
        rpc=HermesRPC(event,request,disconnected)
        await rpc.open(host.sockets[0].getsockname()[1],"fixture-key")
        pending=asyncio.create_task(rpc.call("never.respond",timeout=5))
        await asyncio.wait_for(entered.wait(),2)
        close.set()
        with pytest.raises(RuntimeError,match="連線已中斷"): await pending
        await rpc.reader
        assert rpc.alive is False
        assert len(disconnects)==1
        assert rpc.pending=={}
        await rpc.close()


@pytest.mark.asyncio
async def test_native_learning_notifies_ui_only_after_skill_exists(product):
    agent=bot(product)
    active_task(product,agent)
    product.runtime.skill_baselines[agent]=product.runtime.skill_state(agent)
    event={"type":"tool.complete","session_id":"session","payload":{"name":"skill_manage","tool_id":"learn"}}
    await product.runtime.event(agent,event)
    assert not [e for e in product.store.events(agent) if e["kind"]=="skills_changed"]
    file=product.store.agent_directory(agent)/"hermes/skills/learned-workflow/SKILL.md"
    file.parent.mkdir(parents=True)
    file.write_text("---\nname: learned-workflow\ndescription: When making a report\n---\nVerify the actual output.\n")
    await product.runtime.event(agent,event)
    notice=[e for e in product.store.events(agent) if e["kind"]=="skills_changed"]
    assert len(notice)==1
    assert notice[0]["payload"]["saved"] is True
    assert notice[0]["payload"]["skill_ids"]==["learned-workflow"]
    await product.runtime.event(agent,event)
    assert len([e for e in product.store.events(agent) if e["kind"]=="skills_changed"])==1


def test_scheduler_weekly_and_monthly_are_in_timezone():
    after=datetime(2026,10,7,0,0,tzinfo=timezone.utc)
    assert next_occurrence({"frequency":"weekly","weekdays":[1],"time":"09:00","timezone":"Asia/Taipei"}, after)=="2026-10-12T01:00:00+00:00"
    after=datetime(2026,2,1,0,0,tzinfo=timezone.utc)
    assert next_occurrence({"frequency":"monthly","day_of_month":31,"time":"09:00","timezone":"Asia/Taipei"}, after)=="2026-03-31T01:00:00+00:00"


@pytest.mark.asyncio
async def test_scheduler_repeated_tick_admits_one_occurrence(product):
    agent=bot(product)
    product.runtime.closed=True
    scheduler=Scheduler(product.store, product.runtime)
    routine=uid()
    product.store.put("routines", routine, {"id":routine,"agent_id":agent,"instructions":"collect report","enabled":True,"schedule":{"frequency":"daily","time":"09:00","timezone":"Asia/Taipei"},"next_run_at":"2020-01-01T01:00:00+00:00"})
    await scheduler.tick()
    await scheduler.tick()
    assert len(product.store.all("tasks"))==1


@pytest_asyncio.fixture
async def api_client(product, monkeypatch):
    from server import main
    for field in ["store","settings","runtime","computers"]: monkeypatch.setattr(main, field, getattr(product, field))
    monkeypatch.setattr(main, "skills", Skills(product.store))
    monkeypatch.setattr(main, "scheduler", Scheduler(product.store, product.runtime))
    monkeypatch.setattr(main, "connectors", Connectors(product.store, product.runtime, product.computers))
    product.runtime.closed=True
    transport=httpx.ASGITransport(app=main.app, client=("127.0.0.1",1234))
    async with httpx.AsyncClient(transport=transport, base_url="http://127.0.0.1") as client:
        response=await client.post("/api/agents", json={"name":"API Test"})
        assert response.status_code==200
        yield client, response.json()["id"]


@pytest.mark.asyncio
async def test_http_upload_download_and_path_escape(api_client, tmp_path):
    client, agent=api_client
    uploaded=await client.post(f"/api/agents/{agent}/uploads?name=notes.txt", content=b"test attachment")
    assert uploaded.status_code==200
    response=await client.get(uploaded.json()["url"])
    assert response.content==b"test attachment"
    assert (await client.get(f"/api/agents/{agent}/files/content", params={"path":"../../../settings.json"})).status_code==400


@pytest.mark.asyncio
async def test_clear_conversation_preserves_native_state_and_blocks_queued_work(api_client, product):
    client, agent=api_client
    root=product.store.agent_directory(agent)
    retained={"hermes/memories/MEMORY.md":"private memory","hermes/skills/learned/SKILL.md":"---\nname: learned\n---\n\nVerify results.","workspace/result.txt":"artifact","home/login-state":"browser profile"}
    for relative,content in retained.items():
        path=root/relative;path.parent.mkdir(parents=True,exist_ok=True);path.write_text(content)
    product.store.patch("agents",agent,hermes_session="old-session",live_session="old-live")
    product.store.message(agent,"user","old conversation")
    assert (await client.delete(f"/api/agents/{agent}/conversation")).status_code==200
    assert product.store.transcript(agent)==[]
    assert product.store.get("agents",agent)["hermes_session"] is None
    for relative,content in retained.items(): assert (root/relative).read_text()==content
    await product.runtime.submit(agent,"queued job","queue-before-clear")
    response=await client.delete(f"/api/agents/{agent}/conversation")
    assert response.status_code==400
    assert product.store.transcript(agent)[0]["content"]=="queued job"


@pytest.mark.asyncio
@pytest.mark.parametrize("protocol,endpoint",[("chat_completions","chat/completions"),("responses","responses"),("messages","messages")])
async def test_provider_proxy_uses_bound_credentials_and_native_endpoint(api_client, product, monkeypatch, protocol, endpoint):
    from server import main
    client,agent=api_client
    product.settings.update({"active":"bound","model":"m","provider":{"id":"bound","base_url":"https://provider.example/v1","api_key":"UPSTREAM_ONLY","models":[{"id":"m","protocol":protocol}]}})
    seen=[]
    def upstream(request):
        seen.append(request)
        return httpx.Response(200,json={"ok":True})
    original_client=httpx.AsyncClient
    def model_client(**kwargs): return original_client(transport=httpx.MockTransport(upstream),**kwargs)
    monkeypatch.setattr(main.httpx,"AsyncClient",model_client)
    monkeypatch.setattr(main.computers,"key",lambda id:"WORKER_ONLY",raising=False)
    payload={"model":"m","input":"request"}
    response=await client.post(f"/internal/providers/bound/v1/{endpoint}",headers={"Authorization":"Bearer WORKER_ONLY","anthropic-beta":"fixture-beta"},json=payload)
    assert response.status_code==200
    assert str(seen[0].url)==f"https://provider.example/v1/{endpoint}"
    assert seen[0].headers["Authorization"]=="Bearer UPSTREAM_ONLY"
    assert json.loads(seen[0].content)==payload
    if protocol=="messages":
        assert seen[0].headers["x-api-key"]=="UPSTREAM_ONLY"
        assert seen[0].headers["anthropic-version"]=="2023-06-01"
        assert seen[0].headers["anthropic-beta"]=="fixture-beta"
    assert (await client.post(f"/internal/providers/other/v1/{endpoint}",headers={"Authorization":"Bearer WORKER_ONLY"},json=payload)).status_code==400
    assert len(seen)==1


@pytest.mark.asyncio
async def test_http_import_is_idempotent_and_independent(api_client):
    client, agent=api_client
    template=(await client.get(f"/api/agents/{agent}/template")).json()
    body={"template":template,"name":"Copy","group_id":""}
    first=await client.put("/api/templates/import-same", json=body)
    second=await client.put("/api/templates/import-same", json=body)
    assert first.status_code==second.status_code==200
    assert first.json()["id"]==second.json()["id"]
    assert first.json()["id"]!=agent
    assert first.json().get("transcript",[])==[]


@pytest.mark.asyncio
async def test_http_mcp_ui_remove_and_secret_preservation(api_client):
    client, agent=api_client
    cfg={"action":"save","agent":agent,"server":"fixture","url":"http://example.test/mcp","headers":{"Authorization":"Bearer PRIVATE_TEST_TOKEN"}}
    assert (await client.post("/api/mcp", json=cfg)).status_code==200
    saved=await client.post("/api/mcp", json={**cfg,"headers":{}})
    assert "PRIVATE_TEST_TOKEN" not in saved.text
    from server import main
    assert main.store.get("mcp",agent)["fixture"]["headers"]["Authorization"]=="Bearer PRIVATE_TEST_TOKEN"
    removed=await client.post("/api/mcp", json={"action":"remove","agent":agent,"server":"fixture"})
    assert removed.status_code==200
    assert removed.json()["servers"]==[]


@pytest.mark.asyncio
async def test_http_group_deselection_releases_previous_member(api_client):
    client, agent=api_client
    second=(await client.post("/api/agents", json={"name":"Second"})).json()["id"]
    group=(await client.post("/api/team", json={"action":"create_group","name":"Team","member_ids":[agent,second]})).json()["groups"][0]["id"]
    assert (await client.post("/api/team", json={"action":"update_group","group_id":group,"name":"Team","member_ids":[second]})).status_code==200
    assert not (await client.get(f"/api/agents/{agent}")).json().get("group_id")


@pytest.mark.asyncio
async def test_http_teaching_reports_nothing_until_a_demonstration_starts(api_client):
    client, agent=api_client
    assert (await client.get(f"/api/agents/{agent}/teach")).json()=={"lesson":None}
    assert (await client.post(f"/api/agents/{agent}/teach", json={"action":"note","lesson_id":"missing","text":"x"})).status_code==400
    assert (await client.post(f"/api/agents/{agent}/teach", json={"action":"bogus"})).status_code==400
    assert (await client.post(f"/api/agents/{agent}/lessons", json={})).status_code in {404,405}


@pytest.mark.asyncio
async def test_http_final_answer_absolute_path_opens_in_preview(api_client):
    client, agent=api_client
    uploaded=(await client.post(f"/api/agents/{agent}/uploads?name=report.txt",content=b"verified report")).json()
    from server import main
    upload=main.store.get("uploads",uploaded["id"])
    response=await client.get(f"/api/agents/{agent}/files/content",params={"path":"/workspace/"+upload["path"]})
    assert response.status_code==200
    assert response.content==b"verified report"
    assert (await client.get(f"/api/agents/{agent}/files/content",params={"path":"/workspace/../../settings.json"})).status_code==400


@pytest.mark.asyncio
async def test_http_sharing_scrubs_credentials_and_preserves_required_fields(api_client):
    client, agent=api_client
    from server import main
    main.store.put("mcp",agent,{
        "remote":{"url":"https://user:PRIVATE_PASS@example.test/private-endpoint?token=PRIVATE_QUERY", "headers":{"Authorization":"Bearer PRIVATE_HEADER"},"env":{},"enabled":True},
        "local":{"command":"npx","args":["-y","fixture-mcp","--api-key","PRIVATE_ARGUMENT"],"env":{"TOKEN":"PRIVATE_ENV"},"enabled":True},
    })
    main.store.patch("agents",agent,persona="Token PRIVATE_ENV, API key PRIVATE_ARGUMENT")
    template=(await client.get(f"/api/agents/{agent}/template")).json()
    serialized=json.dumps(template)
    for secret in ["PRIVATE_PASS","PRIVATE_QUERY","PRIVATE_HEADER","PRIVATE_ARGUMENT","PRIVATE_ENV","private-endpoint"]:
        assert secret not in serialized
    assert template["mcp"]["mcpServers"]["remote"]["headers"]=={"Authorization":""}
    assert template["mcp"]["mcpServers"]["local"]["env"]=={"TOKEN":""}
    assert template["mcp"]["mcpServers"]["local"]["args"][:2]==["-y","fixture-mcp"]
    imported=await client.put("/api/templates/portable-import",json={"template":template,"name":"Portable","group_id":""})
    assert imported.status_code==200
    id=imported.json()["id"]
    connectors=(await client.post("/api/mcp",json={"action":"list","agent":id})).json()["servers"]
    assert all(row["enabled"] is False for row in connectors)
    assert next(row for row in connectors if row["server"]=="remote")["missing_fields"]==["header:Authorization"]


@pytest.mark.asyncio
async def test_http_failed_import_leaves_no_partial_bot(api_client):
    client, agent=api_client
    template=(await client.get(f"/api/agents/{agent}/template")).json()
    template["skills"]=[{"name":"","description":"bad","instructions":""}]
    response=await client.put("/api/templates/bad-import",json={"template":template,"name":"Broken","group_id":""})
    assert response.status_code==400
    assert len((await client.get("/api/agents")).json()["agents"])==1


@pytest.mark.asyncio
async def test_text_before_a_tool_call_is_folded_into_narration(product):
    agent=bot(product)
    task=active_task(product,agent)
    delta=lambda text:{"type":"message.delta","session_id":"session","payload":{"text":text}}
    await product.runtime.event(agent,delta("我先搜尋一下。"))
    await product.runtime.event(agent,{"type":"tool.start","session_id":"session","payload":{"name":"web_search","tool_id":"t1"}})
    reply=product.store.transcript(agent)[-1]
    assert reply["content"]=="" and reply["narration"]==["我先搜尋一下。"]
    assert any(e["payload"].get("type")=="narration" for e in product.store.events(agent))
    await product.runtime.event(agent,delta("找到三則新聞。"))
    assert product.store.transcript(agent)[-1]["content"]=="找到三則新聞。"
    product.runtime.finish(agent,task,"answer","")
    final=product.store.transcript(agent)[-1]
    assert final["content"]=="找到三則新聞。"
    assert final["narration"]==["我先搜尋一下。"]


def test_follow_up_about_a_photo_reattaches_the_threads_recent_upload(product):
    agent=bot(product)
    upload={"id":"u1","agent_id":agent,"name":"IMG_1.jpeg","path":"uploads/u1/IMG_1.jpeg","media_type":"image/jpeg","bytes":10,"url":"x"}
    product.store.put("uploads","u1",upload)
    product.store.message(agent,"user","這是什麼照片\n\n[IMG_1.jpeg](/api/agents/a/files/content?path=uploads/u1/IMG_1.jpeg)",thread=agent)
    product.store.message(agent,"assistant","已停止",thread=agent)
    task={"id":"t","agent_id":agent,"thread_id":agent}
    text="我要你看我傳給你的照片"
    assert product.runtime._images(agent,text,task)==["/workspace/uploads/u1/IMG_1.jpeg"]
    assert "/workspace/uploads/u1/IMG_1.jpeg" in product.runtime._prompt(agent,text,task)
    assert product.runtime._images(agent,"今天天氣如何",task)==[]
    fresh=product.runtime._prompt(agent,"看這張 [IMG_1.jpeg](x)",task)
    assert "圖片也直接附在這則訊息" in fresh


@pytest.mark.asyncio
async def test_mcp_list_covers_every_scope_and_remembers_tests(api_client, product, monkeypatch):
    client, agent=api_client
    calls=[]
    async def call(method, params, timeout=None):
        calls.append((method, params))
        if params.get("name")=="broken": raise RuntimeError("MCP 連線逾時")
        return {"ok": True, "tools": [{"name": "query-docs", "description": "Docs"}]}
    fake=SimpleNamespace(alive=True, call=call)
    async def connect(id): return fake
    monkeypatch.setattr(product.runtime, "client", connect)
    await client.post("/api/mcp", json={"action":"save","server":"docs","url":"http://example.test/mcp"})
    await client.post("/api/mcp", json={"action":"save","agent":agent,"server":"broken","url":"http://broken.test/mcp"})
    # No computer is running yet: a background check does not start one.
    skipped=(await client.post("/api/mcp", json={"action":"test","server":"docs","all":True,"auto":True})).json()
    assert skipped["skipped"] and not calls
    product.runtime.clients[agent]=fake
    listing=(await client.post("/api/mcp", json={"action":"test","server":"docs","all":True})).json()["servers"]
    assert [(r["server"], r["scope_name"]) for r in listing]==[("docs","共用"),("broken","API Test")]
    assert listing[0]["last_test"]["ok"] and listing[0]["last_test"]["tools"][0]["name"]=="query-docs"
    failed=(await client.post("/api/mcp", json={"action":"test","agent":agent,"server":"broken","all":True})).json()["servers"]
    assert failed[1]["last_test"]["ok"] is False and "逾時" in failed[1]["last_test"]["error"]
    # Reopening settings shows the remembered results; changing a connector clears its result.
    again=(await client.post("/api/mcp", json={"action":"list","all":True})).json()["servers"]
    assert again[0]["last_test"]["ok"] and again[1]["last_test"]["ok"] is False
    edited=(await client.post("/api/mcp", json={"action":"save","server":"docs","url":"http://example.test/v2","all":True})).json()["servers"]
    assert edited[0]["last_test"] is None


@pytest.mark.asyncio
async def test_http_group_chats_and_bot_messages(api_client, product, monkeypatch):
    from server import main
    from server.collab import Collaboration
    monkeypatch.setattr(main, "collab", Collaboration(product.store, product.runtime))
    client, first=api_client
    second=(await client.post("/api/agents", json={"name":"Writer","title":"文案"})).json()["id"]
    assert (await client.post("/api/rooms", json={"member_ids":[first]})).status_code==400
    room=(await client.post("/api/rooms", json={"member_ids":[first,second]})).json()
    assert room["name"]=="API Test、Writer" and len(room["members"])==2
    posted=(await client.post(f"/api/rooms/{room['id']}/messages", json={"text":"@Writer 寫一段介紹"})).json()
    assert posted["woke"]==["Writer"]
    detail=(await client.get(f"/api/rooms/{room['id']}")).json()
    assert detail["transcript"][0]["content"]=="@Writer 寫一段介紹"
    assert {m["name"]:m["seat"] for m in detail["members"]}=={"API Test":"idle","Writer":"queued"}
    assert (await client.get("/api/rooms")).json()["rooms"][0]["preview"].startswith("你：")
    # The group's hidden seat thread is not one of the bot's own thread tabs.
    assert len((await client.get(f"/api/agents/{second}/threads")).json()["threads"])==1
    renamed=(await client.patch(f"/api/rooms/{room['id']}", json={"name":"發表會小組"})).json()
    assert renamed["name"]=="發表會小組" and not renamed["auto_name"]
    # A bot's own product tool: the directory and an async message, authenticated by its worker key.
    product.computers.key=lambda id: "key-"+id
    headers={"Authorization":"Bearer key-"+first}
    bots=(await client.post("/internal/product", json={"action":"bots"}, headers=headers)).json()["bots"]
    assert [b["name"] for b in bots]==["Writer"]
    sent=(await client.post("/internal/product", json={"action":"message","bot":"Writer","text":"幫忙","intent":"question"}, headers=headers)).json()
    assert sent["sent"] and sent["to"]=="Writer"
    pair=(await client.get(f"/api/agents/{first}/peers/{second}")).json()
    assert pair["messages"][0]["text"]=="幫忙"
    assert (await client.delete(f"/api/rooms/{room['id']}")).json()["ok"]
    assert (await client.get("/api/rooms")).json()["rooms"]==[]


@pytest.mark.asyncio
async def test_http_duplicate_bot_copies_setup_but_not_history(api_client, product):
    client, agent=api_client
    await client.patch(f"/api/agents/{agent}", json={"title":"研究員","persona":"負責查資料","reply_style":"brief"})
    product.skills=Skills(product.store)
    skill_dir=product.store.agent_directory(agent)/"hermes"/"skills"/"my-skill"; skill_dir.mkdir(parents=True, exist_ok=True)
    (skill_dir/"SKILL.md").write_text("---\nname: my-skill\ndescription: x\n---\nDo it")
    product.store.put("mcp", agent, {"docs":{"url":"http://example.test/mcp","headers":{},"env":{}}})
    product.store.message(agent,"user","old conversation",thread=agent)
    copy=(await client.post(f"/api/agents/{agent}/duplicate")).json()
    assert copy["name"]=="API Test 副本" and copy["title"]=="研究員" and copy["reply_style"]=="brief"
    assert (product.store.agent_directory(copy["id"])/"hermes"/"skills"/"my-skill"/"SKILL.md").exists()
    assert product.store.get("mcp", copy["id"])["docs"]["url"]=="http://example.test/mcp"
    assert product.store.transcript(copy["id"])==[]
    again=(await client.post(f"/api/agents/{agent}/duplicate")).json()
    assert again["name"]=="API Test 副本 2"
