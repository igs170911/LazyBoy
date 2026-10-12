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
