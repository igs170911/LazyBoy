import asyncio
import json
from types import SimpleNamespace
import pytest

from server.store import Store, uid, now
from server.config import Settings
from server.computers import Computers
from server.runtime import Runtime
from server import teaching as teaching_module
from server.teaching import Teaching, normalize, lesson_prompt, key_shots


@pytest.fixture
def product(tmp_path):
    store = Store(tmp_path)
    settings = Settings(store)
    computers = Computers(store, settings)
    runtime = Runtime(store, settings, computers)
    id = uid()
    store.put("agents", id, {"id": id, "name": "Test Bot"})
    return store, computers, runtime, id


def field(name, selector, role="textbox"): return {"role": role, "name": name, "selector": selector, "tag": "input"}


def test_steps_read_like_what_the_person_did():
    rows = [
        {"at": 1000, "source": "browser", "kind": "navigate", "url": "https://shop.test/orders", "title": "訂單", "shot": "shots/b-1.jpg"},
        {"at": 2000, "source": "browser", "kind": "input", "target": field("關鍵字", "#q"), "value": "耳", "shot": "shots/b-2.jpg"},
        {"at": 2600, "source": "browser", "kind": "input", "target": field("關鍵字", "#q"), "value": "耳機", "shot": "shots/b-3.jpg"},
        {"at": 3000, "source": "browser", "kind": "input", "target": field("密碼", "#pw"), "value": "hunter2", "secret": True},
        {"at": 4000, "source": "browser", "kind": "click", "target": {"role": "button", "name": "搜尋", "selector": "form > button"}, "shot": "shots/b-3.jpg"},
        {"at": 4005, "source": "browser", "kind": "submit", "target": {"role": "form", "name": ""}},
        {"at": 4500, "source": "browser", "kind": "navigate", "url": "https://shop.test/r?q=x&password=hunter2", "title": "結果"},
        {"at": 9000, "source": "browser", "kind": "tab", "url": "chrome://new-tab-page/"},
        {"at": 10000, "source": "browser", "kind": "navigate", "url": "https://shop.test/report", "title": "報表"},
        {"at": 12000, "source": "desktop", "kind": "click", "window": "試算表", "target": {"role": "menu", "name": "檔案"}},
    ]
    steps = normalize(rows, notes=[{"at": 3500, "text": "日期每次選上個月"}])
    assert [s["text"] for s in steps] == [
        "前往「訂單」",
        "在文字框「關鍵字」輸入「耳機」",
        "在文字框「密碼」輸入「••••」",
        "使用者補充：日期每次選上個月",
        "點擊按鈕「搜尋」 → 進入「結果」",
        "在新分頁前往「報表」",
        "在「試算表」點擊選單「檔案」",
    ]
    assert "hunter2" not in json.dumps(steps, ensure_ascii=False)
    assert steps[4]["result"]["url"] == "https://shop.test/r?q=x&password=••••"
    # Ids come from a step's first row, so a step can be removed while more typing arrives.
    first = normalize(rows[:2])[1]["id"]
    assert steps[1]["id"] == first
    assert [s["text"] for s in normalize(rows, dropped=[first])][1] == "在文字框「密碼」輸入「••••」"


def test_enter_that_submits_a_form_is_one_step():
    rows = [{"at": 1, "source": "browser", "kind": "key", "key": "Enter", "target": field("搜尋", "#s", "searchbox")},
            {"at": 20, "source": "browser", "kind": "submit"},
            {"at": 900, "source": "browser", "kind": "navigate", "url": "https://a.test/?s=x", "title": "搜尋結果"}]
    step, = normalize(rows)
    assert step["kind"] == "submit" and step["text"] == "送出表單 → 進入「搜尋結果」"


def test_the_bot_gets_the_steps_and_what_to_do_with_them():
    lesson = {"id": "L1", "goal": "匯出上月訂單", "trial": False, "steps": normalize([
        {"at": 1, "source": "browser", "kind": "click", "target": {"role": "link", "name": "訂單", "selector": "a.orders"}, "url": "https://s.test", "shot": "shots/b-1.jpg"},
        {"at": 2, "source": "browser", "kind": "select", "target": field("月份", "#m", "combobox"), "value": "九月", "shot": "shots/b-2.jpg"}])}
    prompt = lesson_prompt(lesson)
    assert "匯出上月訂單" in prompt and "1. [瀏覽器] 點擊連結「訂單」" in prompt and "定位 a.orders" in prompt
    assert "截圖 /workspace/lessons/L1/shots/b-1.jpg" in prompt and "skill_manage" in prompt and "clarify" in prompt
    assert "這次不要試跑" in prompt and "lesson: L1" in prompt
    assert key_shots(lesson) == ["/workspace/lessons/L1/shots/b-1.jpg", "/workspace/lessons/L1/shots/b-2.jpg"]


@pytest.fixture
def box(product, monkeypatch):
    """A computer whose recorders start instantly and write what the test gives them."""
    store, computers, runtime, id = product
    calls = []
    async def docker(*args, timeout=45, check=True):
        calls.append(args)
        if "test" in args: return (state.code, "", "")
        if "-d" in args:
            folder = store.agent_directory(id) / "workspace" / args[-1].removeprefix("/workspace/")
            (folder / (".ready-browser" if any(a.endswith("box-teach-browser.mjs") for a in args) else ".ready-desktop")).touch()
        return (0, "", "")
    state = SimpleNamespace(code=0)
    monkeypatch.setattr(teaching_module.computers_module, "docker", docker)
    async def status(_id, refresh=False): return {"ready": True}
    computers.status = status
    kicks = []
    runtime.kick = lambda bot: kicks.append(bot)
    teach = Teaching(store, runtime, computers); teach.STOP_TIMEOUT = 0.05
    return SimpleNamespace(store=store, runtime=runtime, id=id, teach=teach, calls=calls, kicks=kicks, state=state)


async def test_teaching_holds_the_computer_and_hands_the_demo_to_the_bot(box):
    store, runtime, id, teach = box.store, box.runtime, box.id, box.teach
    lesson = await teach.start(id, "查訂單", True)
    assert runtime.teaching[id] == lesson["id"] and runtime.activity(id)["teaching"] == lesson["id"] and runtime.busy(id)
    assert {c[-2].rpartition("/")[2] for c in box.calls if "-d" in c} == {"box-teach-browser.mjs", "box-teach-desktop"}
    # Work sent meanwhile waits: the queue does not start while the human has the computer.
    queued = (await runtime.submit(id, "幫我查天氣"))["queued"]
    await runtime._queue(id)
    assert store.get("tasks", queued)["state"] == "queued"
    folder = teach.folder(id, lesson["id"])
    rows = [{"at": 1000, "source": "browser", "kind": "click", "target": {"role": "button", "name": "訂單"}, "shot": "shots/b-1.jpg"},
            {"at": 2000, "source": "browser", "kind": "click", "target": {"role": "button", "name": "點錯了"}}]
    (folder / "browser.jsonl").write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in rows))
    view = teach.note(id, lesson["id"], "只看已完成")
    assert [s["text"] for s in view["steps"]] == ["點擊按鈕「訂單」", "點擊按鈕「點錯了」", "使用者補充：只看已完成"]
    teach.drop(id, lesson["id"], view["steps"][1]["id"])
    assert teach.pause(id, lesson["id"], True)["state"] == "paused" and (folder / ".paused").exists()
    assert teach.pause(id, lesson["id"], False)["state"] == "recording" and not (folder / ".paused").exists()
    done = await teach.finish(id, lesson["id"])
    assert (folder / ".stop").exists() and id not in runtime.teaching and not runtime.activity(id)["teaching"]
    saved = json.loads((folder / "steps.json").read_text())
    assert [s["text"] for s in saved["steps"]] == ["點擊按鈕「訂單」", "使用者補充：只看已完成"]
    task = store.get("tasks", done["queued"])
    assert task["lesson_id"] == lesson["id"] and store.get("lessons", lesson["id"])["task_id"] == task["id"]
    assert store.transcript(id)[-1]["lesson_id"] == lesson["id"] and "共 2 步" in store.transcript(id)[-1]["content"]
    assert "點擊按鈕「訂單」" in runtime._prompt(id, task["goal"], task) and runtime._images(id, task["goal"], task) == [f"/workspace/lessons/{lesson['id']}/shots/b-1.jpg"]
    assert box.kicks and box.kicks[-1] == id
    with pytest.raises(ValueError): teach.note(id, lesson["id"], "太晚了")


async def test_finishing_before_anything_happened_keeps_teaching(box):
    lesson = await box.teach.start(box.id)
    with pytest.raises(ValueError, match="還沒有記錄到任何操作"): await box.teach.finish(box.id, lesson["id"])
    assert box.runtime.teaching[box.id] == lesson["id"]
    await box.teach.cancel(box.id, lesson["id"])
    assert box.id not in box.runtime.teaching and not box.store.get("lessons", lesson["id"]) and not box.teach.folder(box.id, lesson["id"]).exists()


async def test_teaching_needs_an_idle_bot_and_a_current_computer(box):
    box.state.code = 1
    with pytest.raises(ValueError, match="更新電腦環境"): await box.teach.start(box.id)
    assert box.id not in box.runtime.teaching
    box.state.code = 0
    task = uid(); box.store.put("tasks", task, {"id": task, "agent_id": box.id, "state": "running", "created_at": now()})
    with pytest.raises(ValueError, match="Bot 正在工作"): await box.teach.start(box.id)


async def test_a_restart_discards_an_unfinished_demonstration(box):
    lesson = await box.teach.start(box.id)
    Teaching(box.store, box.runtime, box.runtime.computers).recover()
    assert box.store.get("lessons", lesson["id"])["state"] == "cancelled"
    assert (box.teach.folder(box.id, lesson["id"]) / ".stop").exists()


async def test_the_computer_is_held_while_it_starts_for_a_demonstration(box):
    """Starting the computer can take a minute; queued work must not slip in before the recording begins."""
    store, runtime, id, teach = box.store, box.runtime, box.id, box.teach
    gate = asyncio.Event()
    async def slow_status(_id, refresh=False):
        await gate.wait()
        return {"ready": True}
    teach.computers.status = slow_status
    starting = asyncio.create_task(teach.start(id, "查訂單"))
    await asyncio.sleep(0)
    assert runtime.teaching.get(id)  # held before the computer answers
    queued = (await runtime.submit(id, "幫我查天氣"))["queued"]
    await runtime._queue(id)
    assert store.get("tasks", queued)["state"] == "queued"
    gate.set()
    lesson = await starting
    assert runtime.teaching[id] == lesson["id"]


async def test_a_failed_start_releases_the_computer(box):
    box.state.code = 1  # the computer has no recorder
    with pytest.raises(ValueError): await box.teach.start(box.id)
    assert box.id not in box.runtime.teaching and box.kicks[-1] == box.id


async def test_pausing_a_cancelled_demonstration_is_a_plain_error(box):
    lesson = await box.teach.start(box.id)
    await box.teach.cancel(box.id, lesson["id"])
    with pytest.raises(ValueError, match="已經結束"): box.teach.pause(box.id, lesson["id"], True)


async def test_recorders_that_fail_to_stop_still_end_the_demonstration(box):
    lesson = await box.teach.start(box.id)
    async def broken(id, folder): raise OSError("folder gone")
    box.teach.stop_recorders = broken
    with pytest.raises(OSError): await box.teach.cancel(box.id, lesson["id"])
    assert box.id not in box.runtime.teaching and lesson["id"] not in box.teach.ending and box.kicks[-1] == box.id
    assert box.store.get("lessons", lesson["id"])["state"] == "cancelled"
