"""The bots as an organization: reporting lines, a manager's view of its team, team building and the chairman's approvals."""
import asyncio
from types import SimpleNamespace

import pytest

from server.collab import Collaboration
from server.config import Settings
from server.org import Organization
from server.runtime import Runtime
from server.store import Store, now, uid


@pytest.fixture
def company(tmp_path):
    store = Store(tmp_path)
    computers = SimpleNamespace(prepare=lambda id: store.agent_directory(id), env_dirty=set(), mcp_dirty=set())
    runtime = Runtime(store, Settings(store), computers)
    runtime.closed = True  # nothing actually runs; the tests drive turns by hand
    collab = Collaboration(store, runtime)

    def create(body):
        id = uid(); row = {"id": id, "created_at": now(), **body}
        store.put("agents", id, row); store.agent_directory(id); return row
    collab.create_agent = create
    org = Organization(store, runtime, collab)
    bots = {}
    for name, title in [("CEO", "執行長"), ("Ops", "營運"), ("Dev", "研發"), ("QA", "品質")]:
        bots[name] = create({"name": name, "title": title})["id"]
    for name in ("Ops", "Dev", "QA"): store.patch("agents", bots[name], reports_to=bots["CEO"])
    yield SimpleNamespace(store=store, runtime=runtime, collab=collab, org=org, bots=bots)
    store.conn.close()


def task(company, bot, state="running", **fields):
    key = uid()
    company.store.put("tasks", key, {"id": key, "agent_id": bot, "thread_id": bot, "goal": "x", "state": state, "created_at": now(), **fields})
    if state == "running": company.runtime.active[bot] = key
    return key


def test_reporting_lines_never_loop(company):
    org, b = company.org, company.bots
    assert org.check_manager(b["Dev"], "") == ""
    with pytest.raises(ValueError, match="自己"): org.check_manager(b["Dev"], b["Dev"])
    with pytest.raises(ValueError, match="不能反過來"): org.check_manager(b["CEO"], b["Dev"])
    company.store.patch("agents", b["QA"], reports_to=b["Dev"])
    with pytest.raises(ValueError, match="不能反過來"): org.check_manager(b["Dev"], b["QA"])
    assert {a["name"] for a in org.reports(b["CEO"], deep=True)} == {"Ops", "Dev", "QA"}


def test_each_bot_is_told_its_place_and_the_chairmans_matters(company):
    org, b = company.org, company.bots
    ceo = org.soul_text(b["CEO"])
    assert "你向 使用者（董事長） 報告" in ceo and "Ops（營運）" in ceo and "team_status" in ceo and "create_team" not in ceo
    assert "新商品定價" in ceo and "request_approval" in ceo
    dev = org.soul_text(b["Dev"])
    assert "你向 CEO（執行長） 報告" in dev and "向你報告的有" not in dev
    company.store.patch("agents", b["CEO"], can_manage=True)
    assert "create_team" in org.soul_text(b["CEO"])
    org.set_approval_items([])
    assert "核定" not in org.soul_text(b["Dev"])
    roster = company.collab.roster_text(b["CEO"])
    assert "Dev（研發）［向 CEO 報告］" in roster


def test_a_manager_sees_its_team_without_asking(company):
    org, b = company.org, company.bots
    task(company, b["Dev"], from_agent=b["CEO"], from_name="CEO", goal="[來自隊友 CEO（執行長） 的請求]\n做登入頁\n\n（這則訊息來自另一個 Bot，不是使用者本人）")
    task(company, b["QA"], state="terminal", verdict="completed", ended_at=now(), goal="測試結帳流程", result={"summary": "找到 2 個缺陷"})
    view = {row["name"]: row for row in org.team_status(b["CEO"])["team"]}
    assert set(view) == {"Ops", "Dev", "QA"}
    assert view["Dev"]["state"] == "working" and view["Dev"]["current"] == {"task": "做登入頁", "from": "CEO 交辦", "since": view["Dev"]["current"]["since"]}
    assert view["QA"]["state"] == "idle" and view["QA"]["recent"][0]["result"] == "找到 2 個缺陷" and view["QA"]["recent"][0]["from"] == "使用者交辦"
    assert org.team_status(b["CEO"], "QA")["team"][0]["name"] == "QA"
    # Someone outside the reporting line is not visible, unless the viewer manages the team.
    with pytest.raises(ValueError, match="不在你的團隊"): org.team_status(b["Dev"], "QA")
    assert org.team_status(b["Dev"])["team"] == []
    company.store.patch("agents", b["Dev"], can_manage=True)
    assert len(org.team_status(b["Dev"])["team"]) == 3


@pytest.mark.asyncio
async def test_approvals_wait_for_the_user_and_come_back_as_their_message(company):
    org, b, store = company.org, company.bots, company.store
    origin = task(company, b["Dev"], from_agent=b["CEO"], from_name="CEO")
    sent = org.request_approval(b["Dev"], "採購雲端主機每月 3000 元", "方案 A 與 B 的比較…")
    assert sent["sent"] and org.approvals()[0]["task_id"] == origin
    assert store.transcript(b["Dev"], b["Dev"])[-1]["event"] == "approval_request"
    assert not org.request_approval(b["Dev"], "採購雲端主機每月 3000 元")["sent"]
    submitted = []
    async def submit(id, text, request_id=None, **kwargs): submitted.append((id, text, request_id, kwargs)); return {"queued": "t"}
    company.runtime.submit = submit
    decided = await org.decide(sent["approval_id"], "approve", "上限 3500")
    assert decided["state"] == "approved" and org.approvals() == []
    id, text, request_id, kwargs = submitted[0]
    assert id == b["Dev"] and "【董事長核定】核准：採購雲端主機每月 3000 元" in text and "上限 3500" in text and "CEO" in text
    assert kwargs["thread"] == b["Dev"] and request_id == f"approval-{sent['approval_id']}"
    with pytest.raises(ValueError, match="處理過"): await org.decide(sent["approval_id"], "reject")


def test_a_team_is_built_in_one_step(company):
    org, b, store = company.org, company.bots, company.store
    made = org.create_team("工程團隊", {"name": "技術主管", "title": "主管"}, [{"name": "前端", "title": "前端"}, {"name": "後端"}, "QA"],
                           reports_to="CEO", group_chat=True)
    agents = {a["name"]: a for a in store.all("agents")}
    lead = agents["技術主管"]
    assert lead["reports_to"] == b["CEO"] and lead["group_id"] == made["group_id"]
    assert all(agents[n]["reports_to"] == lead["id"] and agents[n]["group_id"] == made["group_id"] for n in ("前端", "後端", "QA"))
    assert store.get("groups", made["group_id"])["name"] == "工程團隊"
    assert set(store.get("rooms", made["room_id"])["member_ids"]) == {lead["id"], agents["前端"]["id"], agents["後端"]["id"], b["QA"]}
    with pytest.raises(ValueError, match="已經有叫"): org.create_team("第二隊", {"name": "前端"}, [{"name": "新人"}])
    with pytest.raises(ValueError, match="最多 10 位"): org.create_team("大隊", {"name": "隊長"}, [{"name": f"成員{i}"} for i in range(10)])


def test_a_team_that_cannot_be_built_leaves_nothing_behind(company):
    org, b, store = company.org, company.bots, company.store
    before = {a["id"]: a for a in store.all("agents")}
    # The lead listed again as a member, a member nobody knows, and a chart that would loop (the lead's manager is
    # one of its own members): each fails before any section, bot or line changes.
    with pytest.raises(ValueError, match="出現兩次"): org.create_team("營運組", "Ops", ["Ops", {"name": "新人"}])
    with pytest.raises(ValueError, match="找不到"): org.create_team("營運組", "Ops", [{"name": "新人"}, "Nobody"])
    with pytest.raises(ValueError, match="繞成一圈"): org.create_team("營運組", "Ops", ["Dev", {"name": "新人"}], reports_to="Dev")
    assert store.all("groups") == [] and store.all("rooms") == []
    assert {a["id"]: a for a in store.all("agents")} == before


def test_an_approval_from_a_group_chat_turn_is_decided_in_the_bots_own_chat(company):
    org, b, store = company.org, company.bots, company.store
    seat = uid(); store.put("threads", seat, {"id": seat, "agent_id": b["Dev"], "room_id": "room-1", "title": "群組"})
    task(company, b["Dev"], thread_id=seat, room_id="room-1")
    sent = org.request_approval(b["Dev"], "上架新商品")
    assert store.get("approvals", sent["approval_id"])["thread_id"] == b["Dev"]


def test_only_a_bot_with_team_management_changes_others(company):
    org, b, store = company.org, company.bots, company.store
    with pytest.raises(ValueError, match="團隊管理權限"): org.update_bot(b["CEO"], "QA", title="測試主管")
    store.patch("agents", b["CEO"], can_manage=True)
    org.update_bot(b["CEO"], "QA", title="測試主管", reports_to="Dev", team="品質小組")
    qa = store.get("agents", b["QA"])
    assert qa["title"] == "測試主管" and qa["reports_to"] == b["Dev"] and store.get("groups", qa["group_id"])["name"] == "品質小組"
    with pytest.raises(ValueError, match="不能反過來"): org.update_bot(b["CEO"], "Dev", reports_to="QA")
    org.update_bot(b["CEO"], "QA", reports_to="使用者")
    assert store.get("agents", b["QA"])["reports_to"] == ""


def test_a_deleted_managers_team_moves_up(company):
    org, b, store = company.org, company.bots, company.store
    store.patch("agents", b["QA"], reports_to=b["Dev"])
    org.request_approval(b["Dev"], "上架新商品")
    org.forget(b["Dev"])
    assert store.get("agents", b["QA"])["reports_to"] == b["CEO"]
    assert org.approvals() == [] and org.approvals("cancelled")[0]["subject"] == "上架新商品"
