import asyncio
import json
from pathlib import Path
from types import SimpleNamespace
import pytest

from server.store import Store, uid, now
from server.config import Settings
from server.computers import Computers
from server.runtime import Runtime
from server.files import contained
from server.skills import Skills
from server.scheduling import Scheduler, next_occurrence
from datetime import datetime, timezone


@pytest.fixture
def product(tmp_path):
    store = Store(tmp_path)
    settings = Settings(store)
    computers = Computers(store, settings)
    runtime = Runtime(store, settings, computers)
    id = uid()
    store.put("agents", id, {"id": id, "name": "Test Bot"})
    return store, settings, computers, runtime, id


def test_admission_transaction_rolls_back_every_record(product):
    store, _, _, _, id = product
    with pytest.raises(RuntimeError):
        with store.transaction():
            store.admit(id, "same", "task", "work")
            store.put("tasks", "task", {"goal": "work"})
            store.message(id, "user", "work")
            raise RuntimeError("Simulated process failure")
    assert store.admission(id, "same") is None
    assert store.get("tasks", "task") is None
    assert store.transcript(id) == []


async def test_parallel_retries_execute_only_one_admission(product):
    store, _, _, runtime, id = product
    runtime.closed = True  # Admission-only test; the real executor has its own live acceptance.
    results = await asyncio.gather(*(runtime.submit(id, "same work", "one-request") for _ in range(10)))
    assert len({r["queued"] for r in results}) == 1
    assert len(store.all("tasks")) == 1
    assert len(store.transcript(id)) == 1
    with pytest.raises(ValueError): await runtime.submit(id, "different work", "one-request")
    reopened = Store(store.directory)
    assert reopened.admission(id, "one-request")["task"] == results[0]["queued"]


def test_workspace_blocks_cross_bot_and_symlink_access(product, tmp_path):
    store, _, _, _, id = product
    root = store.agent_directory(id) / "workspace"
    outside = tmp_path / "private.txt"; outside.write_text("private")
    (root / "escape").symlink_to(tmp_path, target_is_directory=True)
    for name in ["../../private.txt", "escape/private.txt", str(outside)]:
        with pytest.raises(ValueError): contained(root, name, exists=True)
    (root / "own.txt").write_text("own")
    assert contained(root, "own.txt", exists=True).read_text() == "own"


def test_prepare_preserves_memory_and_secrets_and_removes_deleted_mcp(product):
    store, _, computers, _, id = product
    root = store.agent_directory(id) / "hermes"
    (root / ".env").write_text("MY_SAVED_SECRET=private\n")
    (root / "MEMORY.md").write_text("remember")
    store.put("mcp", id, {"example": {"command": "echo", "args": ["test"]}})
    computers.prepare(id)
    import yaml
    assert "example" in yaml.safe_load((root / "config.yaml").read_text())["mcp_servers"]
    store.put("mcp", id, {})
    computers.prepare(id)
    config = yaml.safe_load((root / "config.yaml").read_text())
    assert "example" not in config["mcp_servers"]
    assert config["mcp_servers"]["computer"]["trust"] == "full"
    assert config["desktop"]["auto_continue"]["enabled"] is False
    assert "memory" in config["platform_toolsets"]["cli"]
    assert "MY_SAVED_SECRET=private" in (root / ".env").read_text()
    assert (root / "MEMORY.md").read_text() == "remember"


async def test_native_multiple_questions_and_secret_never_enter_journal(product):
    store, _, _, runtime, id = product
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id":task,"agent_id":id,"state":"running","created_at":now()})
    responses = []
    async def answer(rid, result): responses.append((rid, result))
    runtime.clients[id] = SimpleNamespace(alive=True, answer=answer)
    await runtime.human_request(id, {"id":"native-q","method":"clarify","params":{"questions":[{"qid":"q0","question":"First?","choices":["A","B"],"multi_select":False},{"qid":"q1","question":"Second?","choices":["C","D"],"multi_select":True}]}})
    assert runtime.activity(id)["state"] == "waiting_input"
    with pytest.raises(ValueError): await runtime.answer(id,"native-q",text="one answer")
    await runtime.answer(id,"native-q",answers={"q0":"B","q1":["C","D"]})
    assert responses[-1][1]["answers"] == {"q0":"B","q1":["C","D"]}
    await runtime.human_request(id, {"id":"secret-q","method":"secret","params":{"env_var":"TEST_KEY","prompt":"API key"}})
    await runtime.answer(id,"secret-q",value="UNIQUE_PRIVATE_SENTINEL")
    assert responses[-1][1]["value"] == "UNIQUE_PRIVATE_SENTINEL"
    assert "UNIQUE_PRIVATE_SENTINEL" not in json.dumps(store.transcript(id))
    assert "UNIQUE_PRIVATE_SENTINEL" not in json.dumps(store.events(id))
    assert "UNIQUE_PRIVATE_SENTINEL" not in json.dumps(store.all("tasks"))
    with pytest.raises(ValueError): await runtime.answer(id,"secret-q",value="late")


async def test_scheduler_restart_catches_up_once(product):
    store, _, _, runtime, id = product; runtime.closed = True
    scheduler = Scheduler(store,runtime)
    routine = {"id":uid(),"agent_id":id,"title":"Daily","instructions":"Do daily work","schedule":{"frequency":"daily","timezone":"Asia/Taipei","time":"09:00"},"enabled":True,"next_run_at":"2020-01-01T00:00:00+00:00"}
    store.put("routines",routine["id"],routine)
    await scheduler.tick()
    assert len(store.all("tasks")) == 1
    # Reproduce a crash after admission committed, before the next timestamp saved.
    store.patch("routines",routine["id"],next_run_at=routine["next_run_at"])
    await Scheduler(store,runtime).tick()
    assert len(store.all("tasks")) == 1
    assert store.get("routines",routine["id"])["next_run_at"] > now()


def test_timezone_weekday_and_invalid_time():
    after = datetime(2026,10,9,2,tzinfo=timezone.utc)  # Friday after 09:00 Taipei.
    assert next_occurrence({"frequency":"weekdays","timezone":"Asia/Taipei","time":"09:00"},after) == "2026-10-12T01:00:00+00:00"
    with pytest.raises(ValueError): next_occurrence({"frequency":"daily","time":"25:00"},after)


def test_weekly_string_days_match_and_empty_days_are_rejected():
    after = datetime(2026,10,9,2,tzinfo=timezone.utc)
    assert next_occurrence({"frequency":"weekly","weekdays":["1","3"],"time":"09:00","timezone":"Asia/Taipei"},after) == "2026-10-12T01:00:00+00:00"
    with pytest.raises(ValueError, match="至少要選一個星期"):
        next_occurrence({"frequency":"weekly","weekdays":[],"time":"09:00","timezone":"Asia/Taipei"},after)


async def test_vault_questions_keep_site_hint_and_identifier_fields(product):
    store, _, _, runtime, id = product
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id":task,"agent_id":id,"state":"running","created_at":now()})
    runtime.clients[id] = SimpleNamespace(alive=True)
    await runtime.human_request(id, {"id":"save-1","method":"vault.save_login","params":{"origin":"https://shop.example","site":"shop.example"}})
    saved = store.get("tasks", task)["question"]
    assert saved["kind"] == "secret"
    assert "shop.example" in saved["prompt"] and "https://shop.example" in saved["prompt"]
    assert [field["name"] for field in saved["fields"]] == ["identifier", "password"]
    await runtime.human_request(id, {"id":"code-1","method":"vault.code","params":{"site":"shop.example","hint":"簡訊驗證碼"}})
    code = store.get("tasks", task)["question"]
    assert "shop.example" in code["prompt"] and "簡訊驗證碼" in code["prompt"]
    assert code["fields"][0]["label"] == "簡訊驗證碼"
    await runtime.human_request(id, {"id":"unlock-1","method":"vault.unlock_prompt","params":{"display_name":"Bitwarden"}})
    unlock = store.get("tasks", task)["question"]
    assert "Bitwarden" in unlock["prompt"]
    assert unlock["fields"][0]["label"] == "Bitwarden"


def test_skills_are_real_hermes_files_and_toggles(product):
    store, _, _, _, id = product; skills = Skills(store)
    rows = skills.manage(id,{"action":"save","name":"Research","instructions":"Read primary sources.","description":"Research method"})
    row = rows[0]; path = store.agent_directory(id)/"hermes"/"skills"/row["id"]/"SKILL.md"
    assert "Read primary sources." in path.read_text()
    assert not skills.manage(id,{"action":"toggle","id":row["id"],"enabled":False})[0]["enabled"]
    with pytest.raises(ValueError): skills.manage(id,{"action":"save","id":"../../elsewhere","name":"Bad","instructions":"Bad"})


def test_tool_failure_follows_hermes_result_not_a_missing_error_flag():
    from server.runtime import tool_failed
    assert tool_failed({"result": {"output": "", "exit_code": 1, "error": None}})
    assert tool_failed({"result": json.dumps({"success": False})})
    assert tool_failed({"result": {"error": "ConnectError"}})
    assert not tool_failed({"result": {"output": "ok", "exit_code": 0}})
    assert not tool_failed({"result": "plain text output"})
    assert not tool_failed({"result": {"exit_code": False}})


def test_prepare_enables_agent_tools_skips_smart_approval_and_writes_search_key(product):
    store, settings, computers, _, id = product
    root = store.agent_directory(id) / "hermes"
    (root / ".env").parent.mkdir(parents=True, exist_ok=True)
    (root / ".env").write_text("MY_SAVED_SECRET=private\nEXA_API_KEY=stale\n")
    settings.update({"search": {"provider": "tavily", "api_key": "tv-key", "xai_api_key": "xai-key"}})
    computers.prepare(id)
    import yaml
    config = yaml.safe_load((root / "config.yaml").read_text())
    tools = config["platform_toolsets"]["cli"]
    assert {"delegation", "code_execution", "session_search", "x_search"} <= set(tools)
    assert "vision" not in tools
    assert config["approvals"]["mode"] == "off"
    assert config["web"]["backend"] == "tavily" and "provider" not in config["web"]
    env = (root / ".env").read_text()
    assert "TAVILY_API_KEY=tv-key" in env and "XAI_API_KEY=xai-key" in env
    assert "EXA_API_KEY=stale" in env and "MY_SAVED_SECRET=private" in env  # not written by LazyBoy: kept
    public = settings.public()["search"]
    assert public == {"provider": "tavily", "providers": ["exa", "tavily", "brave-free", "ddgs"], "has_key": True, "has_xai_key": True}
    assert "tv-key" not in json.dumps(settings.public())
    settings.update({"search": {"provider": "exa"}})
    assert settings.search()["api_key"] == "" and settings.search()["xai_api_key"] == "xai-key"
    with pytest.raises(ValueError): settings.update({"search": {"provider": "google"}})


async def test_failed_tool_reaches_the_step_timeline(product):
    store, _, _, runtime, id = product
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "state": "running", "created_at": now()})
    await runtime.event(id, {"type": "tool.complete", "payload": {"name": "terminal", "tool_id": "t1", "duration_s": 1.5,
                                                                    "summary": "exit 1", "result": {"output": "", "exit_code": 1}}})
    finished = [e for e in store.events(id) if e.get("payload", {}).get("type") == "tool_finished"]
    assert finished[-1]["payload"]["success"] is False and finished[-1]["payload"]["duration"] == 1.5


async def test_finished_turn_keeps_steps_plan_and_sources(product):
    store, _, _, runtime, id = product
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "state": "running", "created_at": now()})
    hit = {"data": {"web": [{"url": "https://a.example/news", "title": "A **news**"}, {"url": "https://b.example", "title": "B"}]}}
    page = {"results": [{"url": "https://b.example", "title": "B page", "content": "x"}, {"url": "https://c.example", "error": "403"}]}
    for tool_id, tool, result in [("s1", "web_search", hit), ("e1", "web_extract", page), ("t1", "terminal", {"exit_code": 2})]:
        await runtime.event(id, {"type": "tool.start", "payload": {"name": tool, "tool_id": tool_id, "context": tool}})
        await runtime.event(id, {"type": "tool.complete", "payload": {"name": tool, "tool_id": tool_id, "result": result, "summary": "boom", "duration_s": 0.25}})
    await runtime.event(id, {"type": "todo.updated", "payload": {"todos": [{"id": "1", "content": "查資料", "status": "completed"}, {"id": "2", "content": "寫報告", "status": "in_progress"}], "revision": 2}})
    assert store.get("tasks", task)["plan"] == [{"content": "查資料", "status": "completed"}, {"content": "寫報告", "status": "in_progress"}]
    runtime.finish(id, task, "answer", "done")
    saved = store.get("tasks", task)
    assert [s["state"] for s in saved["steps"]] == ["done", "done", "failed"]
    assert saved["steps"][2]["detail"] == "boom" and saved["steps"][0]["detail"] == "web_search"
    assert saved["steps"][0]["duration"] == round(0.25, 1)
    assert saved["sources"] == [{"url": "https://b.example", "title": "B", "kind": "read"},
                                {"url": "https://a.example/news", "title": "A news", "kind": "search"}]


async def test_watchdog_interrupts_a_silent_turn_then_fails_it(product):
    store, _, _, runtime, id = product
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "state": "running", "session_id": "s", "created_at": now()})
    calls = []
    async def call(method, params): calls.append(method)
    runtime.clients[id] = SimpleNamespace(alive=True, call=call)
    runtime.finished[task] = asyncio.Event()
    runtime.IDLE_LIMIT, runtime.STOP_GRACE, runtime.WATCH_INTERVAL = 0.05, 0.05, 0.01
    await asyncio.wait_for(runtime.watch(id, task), 2)
    assert calls == ["session.interrupt"]
    saved = store.get("tasks", task)
    assert saved["state"] == "terminal" and saved["verdict"] == "failed"


async def test_watchdog_ends_a_stop_hermes_never_confirms(product):
    store, _, _, runtime, id = product
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "state": "running", "session_id": "s", "created_at": now()})
    async def call(method, params): return {}
    runtime.clients[id] = SimpleNamespace(alive=True, call=call)
    runtime.finished[task] = asyncio.Event()
    runtime.STOP_GRACE, runtime.WATCH_INTERVAL = 0.05, 0.01
    await runtime.stop(id)
    await asyncio.wait_for(runtime.watch(id, task), 2)
    assert store.get("tasks", task)["verdict"] == "cancelled"


async def test_watchdog_never_fires_while_waiting_for_the_user(product):
    store, _, _, runtime, id = product
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "state": "waiting_input", "session_id": "s", "created_at": now()})
    runtime.finished[task] = asyncio.Event()
    runtime.IDLE_LIMIT, runtime.WATCH_INTERVAL = 0.02, 0.01
    watcher = asyncio.create_task(runtime.watch(id, task))
    await asyncio.sleep(0.1)
    assert not watcher.done() and store.get("tasks", task)["state"] == "waiting_input"
    runtime.finished[task].set()
    await asyncio.wait_for(watcher, 1)


async def test_stream_writes_are_throttled_but_every_delta_is_journaled(product):
    store, _, _, runtime, id = product
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "state": "running", "created_at": now()})
    writes = []
    original = store.message
    store.message = lambda *a, **k: writes.append(a) or original(*a, **k)
    for word in ["一", "二", "三", "四"]:
        await runtime.event(id, {"type": "message.delta", "payload": {"text": word}})
    assert len(writes) == 1
    assert [e["payload"]["delta"] for e in store.events(id) if e["payload"].get("type") == "message_delta"] == ["一", "二", "三", "四"]
    runtime.finish(id, task, "answer", "")
    assert store.transcript(id)[-1]["content"] == "一二三四"


async def test_listeners_wake_on_new_events(product):
    store, _, _, runtime, id = product
    with runtime.listen(id) as wake:
        assert not wake.is_set()
        runtime.emit(id, "reply", {})
        assert wake.is_set()
    assert not runtime.listeners[id]


def test_prune_events_keeps_the_recent_journal(product):
    store, _, _, _, id = product
    for _ in range(12): store.event(id, "reply", {})
    store.prune_events(id, keep=5)
    assert len(store.events(id)) == 5


def test_failed_provider_errors_get_a_plain_language_first_line(product):
    store, _, _, runtime, id = product
    task = uid()
    store.put("tasks", task, {"id": task, "agent_id": id, "state": "running", "created_at": now()})
    runtime.finish(id, task, "failed", "custom:product rejected this request as malformed. Run hermes doctor.\n\nProvider said: HTTP 400: Model does not support this protocol.")
    reply = store.transcript(id)[-1]["content"]
    assert reply.startswith("模型的 API 介面設定不對") and "原始錯誤：" in reply and "hermes doctor" in reply


async def test_model_switch_waits_for_a_resumed_agent(product):
    store, settings, computers, runtime, id = product
    settings.update({"provider": {"id": "relay", "base_url": "https://relay.example/v1", "api_key": "k", "models": [{"id": "m", "protocol": "messages"}]}, "active": "relay", "model": "m"})
    calls = []
    async def call(method, params=None, *a, **k):
        calls.append(method)
        if method == "prompt.submit":
            runtime.finish(id, runtime.active[id], "answer", "ok")
        return {}
    client = SimpleNamespace(alive=True, call=call)
    async def no_client(_): return client
    async def no_session(*_): return "sid"
    async def no_files(*_, **__): return []
    runtime.client, runtime.session, runtime.refresh_files = no_client, no_session, no_files
    computers.prepare = lambda _: None
    await runtime.submit(id, "hello", "r1")
    await asyncio.wait_for(runtime.runners[id], 5)
    assert calls.index("approval.pending") < calls.index("config.set")


async def test_watchdog_interrupt_reported_as_failure_not_user_stop(product):
    store, _, _, runtime, id = product
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "state": "running", "created_at": now()})
    runtime.stalled[task] = 0
    await runtime.event(id, {"type": "message.complete", "payload": {"status": "interrupted", "text": ""}})
    saved = store.get("tasks", task)
    assert saved["verdict"] == "failed" and "沒有回應" in store.transcript(id)[-1]["content"]


async def test_stream_tail_is_flushed_without_a_further_delta(product):
    store, _, _, runtime, id = product
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "state": "running", "created_at": now()})
    runtime.PERSIST_INTERVAL = 0.05
    for word in ["一", "二"]:
        await runtime.event(id, {"type": "message.delta", "payload": {"text": word}})
    await asyncio.sleep(0.15)
    assert store.transcript(id)[-1]["content"] == "一二"


def test_env_keeps_keys_lazyboy_did_not_write(product):
    store, settings, computers, _, id = product
    root = store.agent_directory(id) / "hermes"
    (root / ".env").write_text("XAI_API_KEY=user-own\n")
    computers.prepare(id)
    assert "XAI_API_KEY=user-own" in (root / ".env").read_text()
    settings.update({"search": {"provider": "exa", "api_key": "ex"}})
    computers.prepare(id)
    assert "EXA_API_KEY=ex" in (root / ".env").read_text() and id in computers.env_dirty
    settings.update({"search": {"provider": "ddgs"}})
    computers.prepare(id)
    env = (root / ".env").read_text()
    assert "EXA_API_KEY" not in env and "XAI_API_KEY=user-own" in env


def test_prune_is_per_bot(product):
    store, _, _, _, id = product
    other = uid()
    for _ in range(6): store.event(id, "reply", {}); store.event(other, "reply", {})
    store.prune_events(id, keep=4)
    assert len(store.events(id)) == 4 and len(store.events(other)) == 6


def test_deleting_a_bot_leaves_no_records_and_startup_clears_old_orphans(product):
    store, _, _, _, id = product
    other = uid()
    store.put("agents", other, {"id": other, "name": "Keep"})
    for owner in (id, other):
        store.event(owner, "reply", {}); store.admit(owner, "r-" + owner, "t-" + owner, "x"); store.message(owner, "user", "hi")
        store.put("tasks", "t-" + owner, {"id": "t-" + owner, "agent_id": owner, "state": "terminal"})
        store.put("uploads", "u-" + owner, {"id": "u-" + owner, "agent_id": owner}); store.put("files", owner, [])
    store.remove("agents", id)  # deleted the old way: rows left behind
    assert store.purge_orphans() == 1 and store.purge_orphans() == 0
    assert store.events(id) == [] and store.transcript(id) == [] and store.admission(id, "r-" + id) is None
    assert store.get("tasks", "t-" + id) is None and store.get("uploads", "u-" + id) is None and store.get("files", id) is None
    assert store.events(other) and store.transcript(other) and store.get("tasks", "t-" + other) and store.get("files", other) == []
