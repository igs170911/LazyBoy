"""One product scheduler; missed occurrences catch up once, with durable admission."""
import asyncio
import contextlib
from datetime import datetime, timedelta, timezone
import logging
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError
from .store import now, uid

log = logging.getLogger(__name__)


def schedule_zone(schedule):
    # ZoneInfoNotFoundError is a KeyError, which the API would report as a missing record.
    try: return ZoneInfo(schedule.get("timezone", "Asia/Taipei"))
    except (ZoneInfoNotFoundError, ValueError, TypeError): raise ValueError("無效的時區") from None


FREQUENCIES = {"once", "daily", "weekdays", "weekly", "monthly"}
RETRY_MINUTES = (1, 5, 15, 60)


def parse_time(value):
    try:
        parts = [int(x) for x in str(value).strip().split(":")]
        if len(parts) not in (2, 3): raise ValueError
        hour, minute = parts[:2]
    except (TypeError, ValueError): raise ValueError("時間格式要像 09:00") from None
    if not 0 <= hour < 24 or not 0 <= minute < 60: raise ValueError("時間格式要像 09:00")
    return hour, minute


def normalize_schedule(schedule, base=None):
    """Validate and canonicalize; fields left out of an update keep their previous values."""
    if not isinstance(schedule, dict): raise ValueError("無效的排程設定")
    result = {**(base or {}), **{k: v for k, v in schedule.items() if v not in (None, "")}}
    if not result.get("frequency") or not result.get("time"): raise ValueError("請指定重複方式和時間")
    if result["frequency"] not in FREQUENCIES: raise ValueError("無效的排程頻率")
    result["time"] = "%02d:%02d" % parse_time(result["time"])
    result["timezone"] = result.get("timezone") or "Asia/Taipei"
    schedule_zone(result)
    today = datetime.now(schedule_zone(result))
    if result["frequency"] == "weekly":
        raw = result.get("weekdays")
        if isinstance(raw, (str, int)): raw = [raw]
        if not raw: raise ValueError("每週排程至少要選一個星期")
        try: days = sorted({int(str(value).strip()) for value in raw})
        except (TypeError, ValueError): raise ValueError("星期必須介於 1（週一）和 7（週日）") from None
        if not all(1 <= day <= 7 for day in days): raise ValueError("星期必須介於 1（週一）和 7（週日）")
        result["weekdays"] = days
    if result["frequency"] == "monthly":
        # Pin the day now; otherwise each recomputation uses the current day and late runs drift.
        try: day = int(result.get("day_of_month") or today.day)
        except (TypeError, ValueError): raise ValueError("日期必須介於 1 和 31") from None
        if not 1 <= day <= 31: raise ValueError("日期必須介於 1 和 31")
        result["day_of_month"] = day
    if result["frequency"] == "once":
        try: result["date"] = datetime.fromisoformat(str(result.get("date", ""))[:10]).date().isoformat()
        except ValueError: raise ValueError("單次排程必須指定日期（YYYY-MM-DD）") from None
    return result


def next_occurrence(schedule, after=None):
    if not isinstance(schedule, dict): raise ValueError("無效的排程設定")
    zone = schedule_zone(schedule)
    after = after or datetime.now(timezone.utc)
    local = after.astimezone(zone)
    frequency = schedule.get("frequency", "daily")
    if frequency not in FREQUENCIES: raise ValueError("無效的排程頻率")
    hour, minute = parse_time(schedule.get("time", "09:00"))
    if frequency == "once":
        if not isinstance(schedule.get("date"), str): raise ValueError("單次排程必須指定日期")
        candidate = datetime.fromisoformat(schedule["date"][:10]).replace(hour=hour, minute=minute, tzinfo=zone)
        return candidate.astimezone(timezone.utc).isoformat() if candidate > local else None
    weekdays = [int(day) for day in schedule.get("weekdays") or []] if frequency == "weekly" else []
    if frequency == "weekly" and not weekdays: raise ValueError("每週排程至少要選一個星期")
    day = int(schedule.get("day_of_month") or local.day)
    for offset in range(367):
        candidate = (local + timedelta(days=offset)).replace(hour=hour, minute=minute, second=0, microsecond=0)
        if candidate <= local: continue
        if frequency == "weekdays" and candidate.isoweekday() > 5: continue
        if frequency == "weekly" and candidate.isoweekday() not in weekdays: continue
        if frequency == "monthly" and candidate.day != day: continue
        return candidate.astimezone(timezone.utc).isoformat()
    raise ValueError("找不到有效的下次執行時間")


class Scheduler:
    def __init__(self, store, runtime): self.store, self.runtime = store, runtime; self.runner = None

    def list(self, id):
        rows = []
        for row in self.store.all("routines"):
            if row["agent_id"] != id: continue
            task = self.store.get("tasks", row["last_task_id"]) if row.get("last_task_id") else None
            last = {"state": task["state"], "verdict": task.get("verdict"), "at": task.get("started_at") or task["created_at"]} if task else None
            rows.append({**row, "last_run": last})
        return rows

    async def manage(self, id, body):
        action = body.get("action", "list")
        if action == "list": return {"routines": self.list(id)}
        key = body.get("id")
        old = self.store.get("routines", key) if key else None
        if action != "create" and (not old or old["agent_id"] != id): raise KeyError(key)
        if action == "delete": self.store.remove("routines", key); return {}
        if action == "history":
            runs = [{"started_at": t.get("started_at") or t["created_at"], "task_id": t["id"], "state": t["state"], "verdict": t.get("verdict"), "result": t.get("result")} for t in self.store.all("tasks") if t.get("routine_id") == key]
            return {"runs": sorted(runs, key=lambda run: run["started_at"], reverse=True)[:30]}
        if action == "run": return {"task_id": await self.run(old, "manual:" + uid())}
        if action == "pause": return {"routine": self.store.patch("routines", key, enabled=False)}
        if action == "resume":
            upcoming = next_occurrence(old["schedule"])
            if not upcoming: raise ValueError("這個單次排程的時間已經過了，請編輯並設定新的日期")
            return {"routine": self.store.patch("routines", key, enabled=True, next_run_at=upcoming, last_error=None, retry_at=None, failures=0)}
        if action not in {"create", "update"}: raise ValueError("不支援的排程操作")
        # Updates may be partial (the bot often sends only what changed); keep everything else.
        text = lambda name: str(body.get(name) or "").strip() or (old or {}).get(name, "")
        schedule = normalize_schedule(body.get("schedule") or {}, (old or {}).get("schedule") if action == "update" else None)
        enabled = body.get("enabled") if isinstance(body.get("enabled"), bool) else (old or {}).get("enabled", True)
        row = {**(old or {}), "id": key or uid(), "agent_id": id, "title": text("title")[:80], "instructions": text("instructions"), "schedule": schedule, "enabled": enabled}
        if not row["title"] or not row["instructions"]: raise ValueError("請填寫名稱和工作內容")
        row.update(next_run_at=next_occurrence(schedule), last_run_at=row.get("last_run_at"), last_task_id=row.get("last_task_id"), last_error=None, retry_at=None, failures=0)
        if row["enabled"] and not row["next_run_at"]: raise ValueError("單次排程必須是未來的時間")
        self.store.put("routines", row["id"], row)
        return {"routine": row}

    async def run(self, row, occurrence):
        result = await self.runtime.submit(row["agent_id"], row["instructions"], "routine:" + row["id"] + ":" + occurrence, force_queue=True, routine=row["id"])
        task = result["queued"]
        self.store.patch("routines", row["id"], last_run_at=now(), last_task_id=task, last_error=None)
        return task

    async def tick(self):
        for row in self.store.all("routines"):
            due = row.get("next_run_at")
            current_time = datetime.now(timezone.utc)
            if not row.get("enabled") or not due or datetime.fromisoformat(due) > current_time: continue
            if row.get("retry_at") and datetime.fromisoformat(row["retry_at"]) > current_time: continue
            try:
                # The timestamp is the durable idempotency key. A crash between admission
                # and advancement reuses that task rather than firing the occurrence twice.
                await self.run(row, due)
                # Re-read: the routine may have been edited or deleted while admission awaited.
                current = self.store.get("routines", row["id"])
                if not current: continue
                once = current["schedule"].get("frequency", "daily") == "once"
                self.store.patch("routines", row["id"], next_run_at=next_occurrence(current["schedule"]), enabled=current.get("enabled", True) and not once, retry_at=None, failures=0)
            except Exception as exc:
                # Keep the same due time (the idempotency key) but back off instead of retrying every tick.
                failures = int(row.get("failures") or 0) + 1
                retry = current_time + timedelta(minutes=RETRY_MINUTES[min(failures, len(RETRY_MINUTES)) - 1])
                with contextlib.suppress(KeyError): self.store.patch("routines", row["id"], last_error=str(exc)[:500], failures=failures, retry_at=retry.isoformat())

    async def loop(self):
        while True:
            try: await self.tick()
            except Exception: log.exception("Routine scheduler tick failed")
            await asyncio.sleep(2)

    def start(self): self.runner = asyncio.create_task(self.loop())
    async def close(self):
        if self.runner:
            self.runner.cancel(); await asyncio.gather(self.runner, return_exceptions=True)
