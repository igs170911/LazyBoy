from datetime import datetime, timedelta, timezone

from server.config import Settings
from server.computers import Computers
from server.runtime import Runtime
from server.scheduling import Scheduler, next_occurrence
from server.store import Store, uid


def test_month_end_and_year_boundary_keep_local_calendar():
    assert next_occurrence({"frequency":"daily","timezone":"Asia/Taipei","time":"00:00"},datetime(2026,12,31,16,10,tzinfo=timezone.utc)) == "2027-01-01T16:00:00+00:00"
    assert next_occurrence({"frequency":"monthly","day_of_month":31,"timezone":"Asia/Taipei","time":"09:00"},datetime(2026,1,31,2,tzinfo=timezone.utc)) == "2026-03-31T01:00:00+00:00"


async def test_week_of_restarts_and_missed_days_admit_once(tmp_path,monkeypatch):
    from server import scheduling
    clock = datetime(2026,10,7,2,tzinfo=timezone.utc)
    class Clock(datetime):
        @classmethod
        def now(cls,tz=None): return clock.astimezone(tz) if tz else clock.replace(tzinfo=None)
    monkeypatch.setattr(scheduling,"datetime",Clock)
    store=Store(tmp_path);settings=Settings(store);runtime=Runtime(store,settings,Computers(store,settings));runtime.closed=True
    agent=uid();store.put("agents",agent,{"id":agent,"name":"Calendar fixture"})
    routine={"id":uid(),"agent_id":agent,"title":"Daily","instructions":"Calendar proof","enabled":True,"schedule":{"frequency":"daily","timezone":"Asia/Taipei","time":"09:00"},"next_run_at":"2026-10-07T01:00:00+00:00"}
    store.put("routines",routine["id"],routine)
    try:
        for index in range(7):
            clock += timedelta(days=1 if index!=3 else 5)
            due=store.get("routines",routine["id"])["next_run_at"]
            await Scheduler(store,runtime).tick()
            assert len(store.all("tasks"))==index+1
            # Crash after durable admission and before advancing the clock.
            store.patch("routines",routine["id"],next_run_at=due)
            await Scheduler(store,runtime).tick()
            await Scheduler(store,runtime).tick()
            assert len(store.all("tasks"))==index+1
            assert datetime.fromisoformat(store.get("routines",routine["id"])["next_run_at"]) > clock
    finally: store.conn.close()
