"""Routine scheduling: partial updates, validation, backoff and run history."""
from datetime import datetime, timedelta, timezone

import pytest

from server.scheduling import Scheduler, next_occurrence, normalize_schedule
from server.store import Store, uid

WEEKLY = {"frequency": "weekly", "time": "09:00", "timezone": "Asia/Taipei", "weekdays": [1]}


class Runtime:
    def __init__(self, store): self.store, self.calls, self.fail = store, [], False

    async def submit(self, id, text, request_id=None, **fields):
        self.calls.append(request_id)
        if self.fail: raise RuntimeError("電腦啟動失敗")
        task = uid()
        self.store.put("tasks", task, {"id": task, "state": "queued", "created_at": datetime.now(timezone.utc).isoformat(), "routine_id": fields.get("routine")})
        return {"queued": task}


@pytest.fixture
def scheduler(tmp_path):
    store = Store(tmp_path)
    runtime = Runtime(store)
    yield Scheduler(store, runtime), store, runtime
    store.conn.close()


async def create(scheduler, schedule=WEEKLY, **body):
    return (await scheduler.manage("bot", {"action": "create", "title": "週報", "instructions": "整理數據", "schedule": schedule, **body}))["routine"]


async def test_partial_update_keeps_other_fields_and_paused_state(scheduler):
    s, _, _ = scheduler
    routine = await create(s)
    await s.manage("bot", {"action": "pause", "id": routine["id"]})
    updated = (await s.manage("bot", {"action": "update", "id": routine["id"], "title": "", "instructions": "", "schedule": {"time": "10:30"}}))["routine"]
    assert updated["title"] == "週報" and updated["instructions"] == "整理數據"
    assert updated["schedule"] == {**WEEKLY, "time": "10:30"}
    assert updated["enabled"] is False
    renamed = (await s.manage("bot", {"action": "update", "id": routine["id"], "title": "新名稱", "schedule": {}}))["routine"]
    assert renamed["title"] == "新名稱" and renamed["schedule"]["frequency"] == "weekly" and renamed["schedule"]["time"] == "10:30"


@pytest.mark.parametrize("raw, expected", [("9:05", "09:05"), ("09:00:00", "09:00")])
def test_time_is_normalized(raw, expected):
    assert normalize_schedule({"frequency": "daily", "time": raw})["time"] == expected


@pytest.mark.parametrize("schedule, message", [
    ({"frequency": "daily", "time": "9am"}, "時間格式要像 09:00"),
    ({"frequency": "daily"}, "請指定重複方式和時間"),
    ({"frequency": "weekly", "time": "09:00", "weekdays": [0]}, "星期必須介於 1（週一）和 7（週日）"),
    ({"frequency": "once", "time": "09:00", "date": "明天"}, "單次排程必須指定日期"),
    ({"frequency": "daily", "time": "09:00", "timezone": "Mars/Base"}, "無效的時區"),
])
def test_bad_schedules_get_readable_errors(schedule, message):
    with pytest.raises(ValueError, match=message): normalize_schedule(schedule)


def test_weekdays_are_deduplicated_and_sorted():
    assert normalize_schedule({**WEEKLY, "weekdays": ["5", 1, 5]})["weekdays"] == [1, 5]


async def test_failed_admission_backs_off_and_keeps_the_occurrence_key(scheduler):
    s, store, runtime = scheduler
    routine = await create(s, {"frequency": "daily", "time": "09:00", "timezone": "Asia/Taipei"})
    due = "2026-01-01T01:00:00+00:00"
    store.patch("routines", routine["id"], next_run_at=due)
    runtime.fail = True
    for _ in range(5): await s.tick()
    assert len(runtime.calls) == 1
    row = store.get("routines", routine["id"])
    assert row["failures"] == 1 and row["next_run_at"] == due and "電腦啟動失敗" in row["last_error"]
    assert datetime.fromisoformat(row["retry_at"]) > datetime.now(timezone.utc)
    runtime.fail = False
    store.patch("routines", routine["id"], retry_at=(datetime.now(timezone.utc) - timedelta(seconds=1)).isoformat())
    await s.tick()
    assert runtime.calls == [f"routine:{routine['id']}:{due}"] * 2
    row = store.get("routines", routine["id"])
    assert row["failures"] == 0 and row["retry_at"] is None and row["next_run_at"] > due


async def test_finished_once_cannot_be_resumed_without_a_new_date(scheduler):
    s, store, _ = scheduler
    tomorrow = (datetime.now(timezone.utc) + timedelta(days=2)).date().isoformat()
    routine = await create(s, {"frequency": "once", "time": "09:00", "date": tomorrow, "timezone": "Asia/Taipei"})
    store.patch("routines", routine["id"], schedule={**routine["schedule"], "date": "2020-01-01"}, next_run_at="2020-01-01T01:00:00+00:00")
    await s.tick()
    row = store.get("routines", routine["id"])
    assert row["enabled"] is False and row["next_run_at"] is None
    with pytest.raises(ValueError, match="單次排程的時間已經過了"):
        await s.manage("bot", {"action": "resume", "id": routine["id"]})


async def test_list_shows_last_run_and_history_is_newest_first(scheduler):
    s, store, _ = scheduler
    routine = await create(s)
    for when in ("2026-10-01T01:00:00+00:00", "2026-10-08T01:00:00+00:00", "2026-10-05T01:00:00+00:00"):
        task = uid()
        store.put("tasks", task, {"id": task, "state": "terminal", "verdict": "done", "created_at": when, "routine_id": routine["id"]})
    await s.manage("bot", {"action": "run", "id": routine["id"]})
    runs = (await s.manage("bot", {"action": "history", "id": routine["id"]}))["runs"]
    assert [run["started_at"][:10] for run in runs[1:]] == ["2026-10-08", "2026-10-05", "2026-10-01"]
    listed = s.list("bot")[0]
    assert listed["last_run"]["state"] == "queued"


def test_next_occurrence_accepts_a_date_with_time_suffix():
    assert next_occurrence({"frequency": "once", "time": "09:00", "date": "2999-12-01T10:00", "timezone": "Asia/Taipei"}) == "2999-12-01T01:00:00+00:00"


async def test_routine_messages_are_labelled_in_the_transcript(tmp_path):
    from server.computers import Computers
    from server.config import Settings
    from server.runtime import Runtime as Real
    store = Store(tmp_path)
    settings = Settings(store)
    runtime = Real(store, settings, Computers(store, settings))
    runtime.closed = True
    agent = uid()
    store.put("agents", agent, {"id": agent, "name": "Bot", "created_at": "2026-10-08T00:00:00+00:00"})
    s = Scheduler(store, runtime)
    routine = (await s.manage(agent, {"action": "create", "title": "週報", "instructions": "整理數據", "schedule": WEEKLY}))["routine"]
    await s.manage(agent, {"action": "run", "id": routine["id"]})
    message = store.transcript(agent)[-1]
    assert message["routine_id"] == routine["id"] and message["routine_title"] == "週報" and message["content"] == "整理數據"
    store.conn.close()


def test_jitter_runs_each_time_at_a_random_moment_after_the_set_time():
    from datetime import datetime, timezone
    from server.scheduling import normalize_schedule, next_occurrence
    schedule = normalize_schedule({"frequency": "daily", "time": "20:00", "jitter_minutes": 40})
    after = datetime(2026, 10, 13, 1, 0, tzinfo=timezone.utc)  # 09:00 in Taipei
    runs = {next_occurrence(schedule, after) for _ in range(30)}
    slot = datetime(2026, 10, 13, 12, 0, tzinfo=timezone.utc)  # 20:00 in Taipei
    assert len(runs) > 1 and all(0 <= (datetime.fromisoformat(r) - slot).total_seconds() <= 40 * 60 for r in runs)
    assert "jitter_minutes" not in normalize_schedule({"frequency": "daily", "time": "20:00", "jitter_minutes": 0})
    import pytest
    with pytest.raises(ValueError, match="隨機延後"): normalize_schedule({"frequency": "daily", "time": "20:00", "jitter_minutes": 999})
    # Every 30 minutes but up to an hour late would skip slots and really run about once an hour.
    with pytest.raises(ValueError, match="少於 30 分鐘"): normalize_schedule({"frequency": "interval", "every_minutes": 30, "time": "09:00", "jitter_minutes": 60})
    assert normalize_schedule({"frequency": "interval", "every_minutes": 30, "time": "09:00", "jitter_minutes": 20})["jitter_minutes"] == 20
