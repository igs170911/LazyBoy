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
    # Long-lived bots keep their context lean instead of resending old tool output on every call.
    assert config["compression"]["proactive_prune_tokens"] == 48000 and config["compression"]["threshold_tokens"] == 100000
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


async def test_routine_run_tells_the_bot_its_instructions_are_the_permission(product):
    store, _, _, runtime, id = product; runtime.closed = True
    routine = {"id":uid(),"agent_id":id,"title":"Threads 海巡","instructions":"每天回 3 則求推薦貼文","schedule":{"frequency":"daily","timezone":"Asia/Taipei","time":"20:00"},"enabled":True,"next_run_at":"2020-01-01T00:00:00+00:00"}
    store.put("routines",routine["id"],routine)
    await Scheduler(store,runtime).tick()
    task = store.all("tasks")[0]
    prompt = runtime._prompt(id,task["goal"],task)
    assert prompt.startswith("【定時任務「Threads 海巡」時間到了") and prompt.endswith("每天回 3 則求推薦貼文")
    assert "不要停下來問" in prompt and "request_approval" in prompt
    # The routine card still shows only what the user wrote.
    assert task["goal"] == "每天回 3 則求推薦貼文"
    assert not runtime._prompt(id,"你好",{"id":"t","agent_id":id,"thread_id":id}).startswith("【定時任務")


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


def test_prepare_moves_unused_builtin_skills_to_the_library_and_tells_the_bot(product):
    from server.skills import dir_hash
    store, _, computers, _, id = product
    root = store.agent_directory(id) / "hermes" / "skills"
    (root / "productivity/pdf").mkdir(parents=True)
    (root / "productivity/pdf/SKILL.md").write_text("---\nname: pdf\ndescription: PDF files\n---\n\nUse pypdf.")
    (root / ".bundled_manifest").write_text(f"pdf:{dir_hash(root / 'productivity/pdf')}\n")
    computers.prepare(id)
    import yaml
    config = yaml.safe_load((root.parent / "config.yaml").read_text())
    assert config["skills"]["disabled"] == ["pdf"] and config["product_skill_library"] == ["pdf"]
    soul = (root.parent / "SOUL.md").read_text()
    assert "1 個內建技能放在技能庫" in soul and "skill_library" in soul and "不要再建一個相似的" in soul


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


async def test_stream_writes_are_throttled_and_deltas_journaled_in_batches(product):
    store, _, _, runtime, id = product
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "state": "running", "created_at": now()})
    writes = []
    original = store.message
    store.message = lambda *a, **k: writes.append(a) or original(*a, **k)
    deltas = lambda: [e["payload"]["delta"] for e in store.events(id) if e["payload"].get("type") == "message_delta"]
    for word in ["一", "二", "三", "四"]:
        await runtime.event(id, {"type": "message.delta", "payload": {"text": word}})
    assert len(writes) == 1
    # The first words go out with the first save; tokens arriving together after that become one event, not one each.
    assert deltas() == ["一"]
    await asyncio.sleep(runtime.DELTA_INTERVAL * 3)
    assert deltas() == ["一", "二三四"]
    await runtime.event(id, {"type": "message.delta", "payload": {"text": "五"}})
    runtime.finish(id, task, "answer", "")
    # The end of the turn first journals the text still waiting, so nothing arrives after turn_ended.
    assert deltas() == ["一", "二三四", "五"]
    kinds = [e["payload"].get("type") for e in store.events(id) if e["kind"] == "runtime"]
    assert kinds.index("turn_ended") > len(kinds) - 1 - kinds[::-1].index("message_delta")
    assert store.transcript(id)[-1]["content"] == "一二三四五"


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
    from server.model_check import friendly
    assert friendly("custom:product rate-limited every one of 3 attempts — it looks temporarily unavailable. Wait a minute and send /retry").startswith("模型供應商暫時忙碌")


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


async def test_restart_does_not_switch_a_session_to_the_model_it_already_uses(product):
    store, settings, computers, runtime, id = product
    settings.update({"provider": {"id": "relay", "base_url": "https://relay.example/v1", "api_key": "k", "models": [{"id": "m", "protocol": "messages"}, {"id": "n", "protocol": "messages"}]}, "active": "relay", "model": "m"})
    switches = []
    def connect():
        async def call(method, params=None, *a, **k):
            if method == "config.set" and params["key"] == "model": switches.append(params["value"])
            if method == "prompt.submit": runtime.finish(id, runtime.active[id], "answer", "ok")
            return {}
        return SimpleNamespace(alive=True, call=call)
    client = connect()
    async def current_client(_): return client
    async def same_session(*_): return "sid"
    async def no_files(*_, **__): return []
    runtime.client, runtime.session, runtime.refresh_files = current_client, same_session, no_files
    computers.prepare = lambda _: None
    async def turn(text):
        await runtime.submit(id, text, uid())
        await asyncio.wait_for(runtime.runners[id], 5)
    await turn("hello")
    assert switches == ["m --provider custom:product"]
    # The product restarts: a new connection, the same Hermes session, the same model. Switching again would add a
    # "the active model has changed" note the bot answers as if the user had asked about it.
    client = connect()
    await turn("again")
    assert switches == ["m --provider custom:product"]
    settings.update({"model": "n"})
    await turn("on another model")
    assert switches == ["m --provider custom:product", "n --provider custom:product"]


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


async def test_background_workers_outlive_their_turn_and_stop_reaches_them(product):
    store, _, _, runtime, id = product
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "state": "running", "session_id": "sid", "created_at": now()})
    spawn = {"goal": "調查 **Threads** 熱門話題", "task_count": 2, "task_index": 0, "delegation_id": "deleg_1", "depth": 1}
    await runtime.event(id, {"type": "subagent.spawn_requested", "session_id": "sid", "payload": spawn})
    await runtime.event(id, {"type": "subagent.start", "session_id": "sid", "payload": {**spawn, "subagent_id": "sa-1"}})
    runtime.finish(id, task, "answer", "三線研究跑中")
    runtime.release(id, task)
    # The turn ended; the child keeps working and reports progress to the same task.
    await runtime.event(id, {"type": "subagent.tool", "session_id": "sid", "payload": {**spawn, "tool_name": "web_extract", "tool_preview": "https://www.threads.net"}})
    worker, = store.get("tasks", task)["background"]
    assert worker["status"] == "running" and worker["tool"] == "web_extract" and worker["goal"] == "調查 Threads 熱門話題" and worker["background"]
    activity = runtime.activity(id)
    assert activity["state"] == "idle" and activity["background"] == 1 and activity["background_task_ids"] == [task] and runtime.busy(id)
    calls = []
    async def call(method, params): calls.append((method, params))
    runtime.clients[id] = SimpleNamespace(alive=True, call=call)
    assert (await runtime.stop(id))["status"] == "stopping" and calls == [("session.interrupt", {"session_id": "sid"})]
    await runtime.event(id, {"type": "subagent.complete", "session_id": "sid", "payload": {**spawn, "status": "interrupted", "duration_seconds": 12.34}})
    worker, = store.get("tasks", task)["background"]
    assert worker["status"] == "interrupted" and worker["duration"] == 12.3 and "tool" not in worker
    assert runtime.activity(id)["background"] == 0 and not runtime.busy(id)


async def test_lost_connection_does_not_leave_workers_running_forever(product):
    store, _, _, runtime, id = product
    task = uid()
    store.put("tasks", task, {"id": task, "agent_id": id, "state": "terminal", "session_id": "sid", "created_at": now(),
                              "background": [{"id": "k", "goal": "g", "status": "running"}, {"id": "d", "goal": "g", "status": "completed"}]})
    await runtime.disconnected(id, "gone")
    assert [w["status"] for w in store.get("tasks", task)["background"]] == ["lost", "completed"]


async def test_a_turn_hermes_starts_itself_reaches_the_conversation(product):
    store, _, _, runtime, id = product
    runtime.clients[id] = SimpleNamespace(alive=True, product_session="sid")
    await runtime.event(id, {"type": "status.update", "session_id": "sid", "payload": {"kind": "process", "text": "背景工作完成"}})
    await runtime.event(id, {"type": "message.start", "session_id": "sid", "payload": {}})
    task = runtime.active[id]
    assert store.get("tasks", task)["kind"] == "background" and store.get("tasks", task)["thread_id"] == id
    # Work queued meanwhile waits for Hermes's own turn instead of colliding with it.
    queued = await runtime.submit(id, "下一件事", "r1", force_queue=True)
    await asyncio.sleep(0.05)
    assert store.get("tasks", queued["queued"])["state"] == "queued"
    runtime.kick = lambda _: None
    await runtime.event(id, {"type": "message.delta", "session_id": "sid", "payload": {"text": "計畫如下"}})
    await runtime.event(id, {"type": "message.complete", "session_id": "sid", "payload": {"text": "計畫如下：…"}})
    await asyncio.wait_for(runtime.adopted[id], 2)
    saved = store.get("tasks", task)
    assert saved["state"] == "terminal" and saved["verdict"] == "answer" and id not in runtime.active
    assert any(m["content"] == "計畫如下：…" for m in store.transcript(id))
    runtime.runners[id].cancel()


async def test_an_unknown_session_never_becomes_a_task(product):
    store, _, _, runtime, id = product
    runtime.clients[id] = SimpleNamespace(alive=True, product_session="sid")
    await runtime.event(id, {"type": "message.start", "session_id": "someone-else", "payload": {}})
    assert id not in runtime.active and not store.all("tasks")


def test_only_a_stopped_turn_cuts_off_model_requests(product):
    store, _, _, runtime, id = product
    for task, state, verdict in [("a", "terminal", "answer"), ("b", "terminal", "cancelled"), ("c", "stopping", None), ("d", "running", None)]:
        store.put("tasks", task, {"id": task, "agent_id": id, "state": state, "verdict": verdict})
    assert [runtime.halted(t) for t in "abcd"] == [False, True, True, False] and not runtime.halted(None)


class FakeClient:
    alive = True
    def __init__(self): self.calls = []; self.answers = []
    async def call(self, method, params=None, **_):
        self.calls.append((method, params))
        return {"status": "queued"}
    async def answer(self, rid, result): self.answers.append((rid, result))


async def test_a_picture_sent_mid_turn_waits_for_its_own_turn(product):
    store, _, _, runtime, id = product
    runtime.closed = True
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "thread_id": id, "state": "running", "session_id": "s", "created_at": now()})
    client = runtime.clients[id] = FakeClient()
    store.put("uploads", "u1", {"id": "u1", "agent_id": id, "name": "photo.png", "path": "uploads/photo.png", "media_type": "image/png"})
    result = await runtime.submit(id, "看這張 [photo.png](/api/x/u1)")
    assert "queued" in result and not [c for c in client.calls if c[0] == "session.steer"]
    steered = await runtime.submit(id, "順便查一下價格")
    assert steered == {"steered": task}


async def test_answers_show_what_was_chosen(product):
    store, _, _, runtime, id = product
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "thread_id": id, "state": "waiting_approval", "created_at": now()})
    client = runtime.clients[id] = FakeClient()
    runtime.pending_requests[id] = {"id": 7, "method": "approval", "params": {}}
    await runtime.answer(id, 7, choice="always")
    runtime.pending_requests[id] = {"id": 8, "method": "clarify", "params": {"questions": [{"qid": "a", "question": "要寄給誰？"}, {"qid": "b", "question": "什麼時候？"}]}}
    await runtime.answer(id, 8, answers={"a": "王經理", "b": "明天早上"})
    assert [m["content"] for m in store.transcript(id)] == ["一律允許", "- 要寄給誰？：王經理\n- 什麼時候？：明天早上"]
    assert client.answers[0] == (7, {"choice": "always"})


async def test_background_report_in_another_thread_is_kept(product):
    store, _, _, runtime, id = product
    side = uid()
    store.put("threads", side, {"id": side, "agent_id": id, "title": "研究"})
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "thread_id": id, "state": "running", "session_id": "main-sid", "created_at": now()})
    client = runtime.clients[id] = FakeClient()
    client.product_sessions = {side: "side-sid"}
    await runtime.event(id, {"type": "message.complete", "session_id": "side-sid", "payload": {"text": "背景研究完成：三個來源都確認了"}})
    assert [(m["thread"], m["content"]) for m in store.transcript(id)] == [(side, "背景研究完成：三個來源都確認了")]
    assert store.get("tasks", task)["state"] == "running"


def test_reply_for_a_deleted_group_chat_stays_silent(product):
    store, _, _, runtime, id = product
    task = uid()
    store.put("tasks", task, {"id": task, "agent_id": id, "thread_id": id, "room_id": "gone", "state": "running", "created_at": now()})
    runtime.finish(id, task, "cancelled", "已停止")
    assert store.transcript(id)[-1]["silent"] is True


async def test_a_saved_stream_says_which_events_it_already_holds(product):
    store, _, _, runtime, id = product
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "state": "running", "created_at": now()})
    await runtime.event(id, {"type": "message.delta", "payload": {"text": "第一段"}})
    saved = store.transcript(id)[-1]
    delta_events = [e for e in store.events(id) if e["payload"].get("type") == "message_delta"]
    # Everything in the saved copy has been journaled, up to the event it names.
    assert saved["content"] == "第一段" and delta_events and delta_events[-1]["id"] <= saved["stream_event"]
    assert "".join(e["payload"]["delta"] for e in delta_events) == saved["content"]


async def test_late_events_of_a_turn_given_up_on_do_not_reach_the_next_task(product):
    store, _, _, runtime, id = product
    old, new = uid(), uid()
    store.put("tasks", old, {"id": old, "agent_id": id, "thread_id": id, "state": "stopping", "session_id": "s", "created_at": now()})
    runtime.active[id] = old; runtime.finished[old] = asyncio.Event()
    runtime.STOP_GRACE, runtime.WATCH_INTERVAL = 0, 0.01
    runtime.stalled[old] = 0
    await asyncio.wait_for(runtime.watch(id, old), 1)
    assert store.get("tasks", old)["verdict"] == "cancelled" and runtime.draining == {"s": True}
    runtime.release(id, old)
    store.put("tasks", new, {"id": new, "agent_id": id, "thread_id": id, "state": "running", "session_id": "s", "created_at": now()})
    runtime.active[id] = new; runtime.finished[new] = asyncio.Event()
    await runtime.event(id, {"type": "message.delta", "session_id": "s", "payload": {"text": "舊的回覆"}})
    await runtime.event(id, {"type": "message.complete", "session_id": "s", "payload": {"text": "舊的回覆"}})
    assert store.get("tasks", new)["state"] == "running" and "s" not in runtime.draining
    await runtime.event(id, {"type": "message.start", "session_id": "s", "payload": {}})
    await runtime.event(id, {"type": "message.complete", "session_id": "s", "payload": {"text": "新的回覆"}})
    assert store.get("tasks", new)["state"] == "terminal" and store.transcript(id)[-1]["content"] == "新的回覆"


def test_daily_cleanup_prunes_model_usage_with_the_usage_module():
    from server import cleanup, usage
    assert cleanup.model_usage.prune is usage.prune and callable(cleanup.usage)


def test_ask_first_boundaries_are_written_into_the_bots_instructions(product):
    store, _, computers, _, id = product
    store.patch("agents", id, ask_first=["pay", "send"])
    soul = (computers.prepare(id) / "hermes" / "SOUL.md").read_text()
    assert "一定先取得使用者同意" in soul and "付款、購買" in soul and "寄出或傳送訊息" in soul and "公開發佈" not in soul
    # Unattended work asks through the approvals inbox, which never expires.
    assert "request_approval" in soul.split("一定先取得使用者同意")[1].split("。\n")[0] or "改用 request_approval" in soul
    store.patch("agents", id, ask_first=[])
    assert "一定先取得使用者同意" not in (computers.prepare(id) / "hermes" / "SOUL.md").read_text()


@pytest.mark.asyncio
async def test_a_turn_that_stops_with_its_plan_unfinished_carries_on_by_itself(product):
    store, _, _, runtime, id = product
    runtime.closed = False
    queued = []
    async def submit(agent, text, request_id=None, **kwargs): queued.append((text, request_id, kwargs)); return {"queued": "next"}
    runtime.submit = submit
    plan = [{"content": "挑出 3 個商品", "status": "completed"}, {"content": "找 Threads 新貼文", "status": "pending"}]
    def finished(text, **fields):
        key = uid()
        store.put("tasks", key, {"id": key, "agent_id": id, "thread_id": id, "goal": "衝分潤", "state": "running", "created_at": now(), "plan": plan, **fields})
        runtime.finish(id, key, "answer", text)
        return key
    first = finished("商品挑好了，Threads 還沒找到。要我繼續嗎？")
    await asyncio.sleep(0)
    text, request_id, kwargs = queued.pop()
    assert "找 Threads 新貼文" in text and "不要問使用者要不要繼續" in text and request_id == f"continue-{first}"
    assert kwargs["auto"]["depth"] == 1 and kwargs["auto"]["shown"] == "自動繼續：計畫還有 1 項沒完成" and kwargs["force_queue"]
    # No plan, but the work was handed back to the person: it carries on too. A question about direction does not.
    plan = []
    finished("短鏈讀不到，請你在蝦皮後台手動點一次取得連結，把短鏈貼給我")
    finished("你想走省錢路線，還是嘴碎路線？")
    # A login mentioned in passing earlier in the report does not stop it; one asked for at the end does.
    finished("未完成：廣告要你登入 Meta。\n\n需要你做的一小步：\n請在電腦上點回覆列、貼上草稿，按送出，然後按交還給 Bot。\n\n另外：\n- Meta 廣告活動需要你登入後建立。")
    await asyncio.sleep(0)
    assert len(queued) == 2 and all(item[2]["auto"]["shown"] == "自動繼續：自己把剩下的做完" for item in queued)
    queued.clear()
    plan = [{"content": "找 Threads 新貼文", "status": "pending"}]
    # Needing the person, a turn already continued twice, or a new message from the user: it stops and reports.
    finished("需要你先登入 Threads 才能繼續")
    # Asking for the go-ahead before posting or sending is the person's call, even worded as a hand-back.
    finished("三則留言的草稿寫好了，請你確認內容後我再發。")
    finished("草稿在 /workspace/reports/drafts.md，要不要發出去？")
    finished("信件已經擬好，請你看一下，可以寄出去嗎？")
    finished("還差一點", auto_continue=2)
    store.put("tasks", "user-next", {"id": "user-next", "agent_id": id, "thread_id": id, "goal": "改一下", "state": "queued", "created_at": now()})
    finished("還差一點")
    await asyncio.sleep(0)
    assert queued == []


async def test_stop_leaves_an_ended_turn_alone_and_survives_a_failed_interrupt(product):
    store, _, _, runtime, id = product
    runtime.closed = True
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "thread_id": id, "state": "terminal", "verdict": "answer", "session_id": "s", "created_at": now()})
    client = runtime.clients[id] = FakeClient()
    assert await runtime.stop(id) == {"status": "idle"}
    assert store.get("tasks", task)["state"] == "terminal" and store.get("tasks", task)["verdict"] == "answer" and not client.calls
    store.patch("tasks", task, state="running", verdict=None)
    async def broken(method, params=None, **_): raise RuntimeError("Hermes RPC error")
    client.call = broken
    assert await runtime.stop(id) == {"status": "stopping"}
    assert store.get("tasks", task)["state"] == "stopping" and runtime.halt_event(task).is_set()


async def test_only_clarify_and_approval_words_answer_a_waiting_question_from_the_chat(product):
    store, _, _, runtime, id = product
    runtime.closed = True
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "thread_id": id, "state": "running", "session_id": "s", "created_at": now()})
    client = runtime.clients[id] = FakeClient()
    # A secret prompt: chat text is a new message, never sent to Hermes as an empty secret.
    await runtime.human_request(id, {"id": "q1", "method": "secret", "params": {"prompt": "API key"}})
    assert "queued" in await runtime.submit(id, "等一下，金鑰我晚點給")
    assert client.answers == [] and runtime.pending_requests[id]["id"] == "q1" and not [c for c in client.calls if c[0] == "session.steer"]
    # An approval: anything but an approval word waits as a message instead of becoming a denial.
    await runtime.human_request(id, {"id": "q2", "method": "approval", "params": {"command": "npm i", "choices": ["once", "deny"]}})
    assert "queued" in await runtime.submit(id, "先說明一下這個指令要做什麼")
    assert client.answers == []
    assert await runtime.submit(id, "允許") == {"ok": True} and client.answers == [("q2", {"choice": "once"})]
    await runtime.human_request(id, {"id": "q3", "method": "clarify", "params": {"questions": [{"qid": "a", "question": "寄給誰？"}]}})
    assert await runtime.submit(id, "王經理") == {"ok": True} and client.answers[-1] == ("q3", {"answers": {"a": "王經理"}, "outcome": "answered"})


async def test_another_sessions_turn_during_setup_does_not_finish_the_new_task(product):
    store, _, _, runtime, id = product
    runtime.closed = True
    task = uid(); runtime.active[id] = task; runtime.finished[task] = asyncio.Event()
    # Setup has not yet stored the task's session; a background delegate reports back on the main session meanwhile.
    store.put("tasks", task, {"id": task, "agent_id": id, "thread_id": id, "state": "running", "created_at": now()})
    runtime.clients[id] = SimpleNamespace(alive=True, product_session="main-sid")
    for name, payload in [("message.start", {}), ("message.delta", {"text": "背景結果"}), ("message.complete", {"text": "背景工作的結果"})]:
        await runtime.event(id, {"type": name, "session_id": "main-sid", "payload": payload})
    assert store.get("tasks", task)["state"] == "running" and not runtime.finished[task].is_set() and task not in runtime.streams
    assert [m["content"] for m in store.transcript(id) if m.get("background_report")] == ["背景工作的結果"]


async def test_a_stop_before_the_computer_is_ready_starts_nothing(product, monkeypatch):
    store, _, _, runtime, id = product
    runtime.closed = True
    route = SimpleNamespace(primary=lambda: ("p", "m", {}), intent=None, role=None, enabled=False)
    started = []
    async def select(*args, **kwargs):
        await runtime.stop(id); return route
    async def client(_id): started.append(_id)
    monkeypatch.setattr(runtime.router, "select", select)
    monkeypatch.setattr(runtime, "client", client)
    queued = (await runtime.submit(id, "查天氣"))["queued"]
    runtime.closed = False
    await runtime._queue(id)
    assert store.get("tasks", queued)["verdict"] == "cancelled" and started == [] and id not in runtime.active
    # Stopped while the connection opened: no session is created.
    fake = FakeClient()
    async def stop_while_connecting(_id):
        await runtime.stop(id); return fake
    async def ready(*args, **kwargs): return route
    monkeypatch.setattr(runtime.router, "select", ready)
    monkeypatch.setattr(runtime, "client", stop_while_connecting)
    runtime.closed = True; queued = (await runtime.submit(id, "查股價"))["queued"]; runtime.closed = False
    await runtime._queue(id)
    assert store.get("tasks", queued)["verdict"] == "cancelled" and fake.calls == []


def test_a_multi_select_answer_reads_as_a_list_of_choices():
    frame = {"method": "clarify", "params": {"questions": [{"qid": "a", "question": "要哪些平台？"}, {"qid": "b", "question": "語氣？"}]}}
    text = Runtime.answer_text(frame, answers={"a": ["Threads (Recommended)", "IG"], "b": "輕鬆（Recommended）"})
    assert text == "- 要哪些平台？：Threads、IG\n- 語氣？：輕鬆"
    assert Runtime.answer_text({"method": "clarify", "params": {"questions": [{"qid": "a"}]}}, "明天 (Recommended)") == "明天"


async def test_forgetting_a_deleted_bot_drops_its_per_bot_state(product):
    store, _, _, runtime, id = product
    runtime.closed = True
    turn = asyncio.create_task(asyncio.sleep(60)); runtime.adopted[id] = turn
    runtime.connect_locks[id] = asyncio.Lock(); runtime.admission_locks[id] = asyncio.Lock(); runtime.listeners[id] = set()
    runtime.last_bindings[id] = ("p", "m", {}); runtime.skill_baselines[id] = {}; runtime.teaching[id] = "lesson"
    store.put("tasks", "t1", {"id": "t1", "agent_id": id, "state": "terminal", "created_at": now()})
    store.put("tasks", "other", {"id": "other", "agent_id": "someone", "state": "running", "created_at": now()})
    runtime.worker_owner.update({"w1": "t1", "w2": "other"})
    await runtime.forget(id)
    assert turn.cancelled() and id not in runtime.adopted and runtime.worker_owner == {"w2": "other"}
    assert all(id not in table for table in (runtime.connect_locks, runtime.admission_locks, runtime.listeners, runtime.last_bindings, runtime.skill_baselines, runtime.teaching))


def test_release_forgets_workers_of_turns_that_are_done(product):
    store, _, _, runtime, id = product
    store.put("tasks", "done", {"id": "done", "agent_id": id, "state": "terminal", "created_at": now(), "background": [{"id": "w1", "status": "completed"}]})
    store.put("tasks", "busy", {"id": "busy", "agent_id": id, "state": "terminal", "created_at": now(), "background": [{"id": "w2", "status": "running"}]})
    runtime.worker_owner.update({"w1": "done", "w2": "busy", "w3": "gone"})
    runtime.release(id, "done")
    assert runtime.worker_owner == {"w2": "busy"}


def test_a_null_base_url_keeps_the_saved_address(product):
    _, settings, _, _, _ = product
    settings.update({"provider": {"id": "p", "base_url": "https://api.example/v1", "api_key": "k"}})
    settings.update({"provider": {"id": "p", "base_url": None}})
    assert settings.read()["providers"]["p"]["base_url"] == "https://api.example/v1"
