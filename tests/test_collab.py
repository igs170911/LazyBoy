"""Bots working as a team: messages between bots, group chats, and the limits that keep them from running away."""
import asyncio
from types import SimpleNamespace

import pytest

from server import collab as collab_module
from server.collab import Collaboration, mentions
from server.config import Settings
from server.runtime import Runtime
from server.store import Store, uid


@pytest.fixture
def team(tmp_path):
    store = Store(tmp_path)
    computers = SimpleNamespace(prepare=lambda id: store.agent_directory(id), env_dirty=set(), mcp_dirty=set())
    runtime = Runtime(store, Settings(store), computers)
    runtime.closed = True  # nothing actually runs; the tests drive turns by hand
    collab = Collaboration(store, runtime)
    bots = {}
    for name, title in [("Chief", "總管"), ("Writer", "文案"), ("Scout", "研究")]:
        id = uid(); store.put("agents", id, {"id": id, "name": name, "title": title, "persona": f"{name} 的專長"}); store.agent_directory(id)
        bots[name] = id
    yield SimpleNamespace(store=store, runtime=runtime, collab=collab, bots=bots)
    store.conn.close()


def running(team, bot, **fields):
    """Pretend `bot` is in the middle of a turn started by the user (or by `fields`)."""
    task = uid()
    team.store.put("tasks", task, {"id": task, "agent_id": bot, "thread_id": bot, "goal": "x", "state": "running", **fields})
    team.runtime.active[bot] = task
    return task


def queued(team, bot):
    return [t for t in team.store.all("tasks") if t["agent_id"] == bot and t["state"] == "queued"]


async def settle():
    for _ in range(5): await asyncio.sleep(0)


def test_roster_lists_teammates_but_not_the_bot_itself(team):
    chief = team.bots["Chief"]
    names = [m["name"] for m in team.collab.directory(chief)]
    assert names == ["Writer", "Scout"]
    roster = team.collab.roster_text(chief)
    assert "Writer（文案）" in roster and "message_bot" in roster and "Chief（" not in roster
    assert team.collab.resolve("@writer", chief)["id"] == team.bots["Writer"]
    with pytest.raises(ValueError, match="找不到隊友"): team.collab.resolve("Nobody", chief)


@pytest.mark.asyncio
async def test_a_request_wakes_the_recipient_and_its_answer_comes_back(team):
    chief, writer = team.bots["Chief"], team.bots["Writer"]
    origin = running(team, chief)
    sent = await team.collab.send(chief, "Writer", "寫一段新品文案", "request")
    assert sent["sent"] and "自動" in sent["note"]
    # The sender's chat shows "已傳訊息給 Writer"; the recipient gets a low-priority turn in its own chat.
    event = team.store.transcript(chief, chief)[-1]
    assert event["role"] == "event" and event["event"] == "message_out" and event["peer_name"] == "Writer"
    [task] = queued(team, writer)
    assert task["from_agent"] == chief and task["intent"] == "request" and task["low_priority"] and task["parent_task"] == origin
    assert "不是使用者本人" in task["goal"]
    shown = team.store.transcript(writer, writer)[-1]
    assert shown["content"] == "寫一段新品文案" and shown["from_agent"] == chief and shown["event"] == "message_in"
    # Writer answers: the answer returns to Chief as a result, in the conversation the request came from.
    team.store.patch("tasks", task["id"], state="running"); team.runtime.active[writer] = task["id"]
    team.runtime.finish(writer, task["id"], "answer", "文案在 /workspace/copy.md")
    await settle()
    [back] = queued(team, chief)
    assert back["from_agent"] == writer and back["intent"] == "result" and back["thread_id"] == chief and back["hop"] == 2
    # A result never asks for another reply: finishing it sends nothing back to Writer.
    team.store.patch("tasks", back["id"], state="running"); team.runtime.active[chief] = back["id"]
    team.runtime.finish(chief, back["id"], "answer", "已整合給使用者")
    await settle()
    assert not queued(team, writer)
    pair = team.store.all("bot_messages")
    assert [(m["from_name"], m["intent"]) for m in pair] == [("Chief", "request"), ("Writer", "result")]


@pytest.mark.asyncio
async def test_silent_answers_are_hidden_and_not_sent_back(team):
    chief, writer = team.bots["Chief"], team.bots["Writer"]
    running(team, chief)
    await team.collab.send(chief, "Writer", "有空看一下", "question")
    [task] = queued(team, writer)
    team.store.patch("tasks", task["id"], state="running"); team.runtime.active[writer] = task["id"]
    team.runtime.finish(writer, task["id"], "answer", "[silent]")
    await settle()
    assert team.store.transcript(writer, writer)[-1]["silent"] is True
    assert not queued(team, chief)


@pytest.mark.asyncio
async def test_the_users_messages_go_before_bot_messages(team):
    chief, writer = team.bots["Chief"], team.bots["Writer"]
    running(team, chief)
    await team.collab.send(chief, "Writer", "幫忙", "request")
    team.runtime.active.pop(writer, None)
    await team.runtime.submit(writer, "使用者自己的問題", force_queue=True)
    assert team.runtime.next_task(writer)["goal"] == "使用者自己的問題"


@pytest.mark.asyncio
async def test_an_exchange_stops_after_max_hops(team):
    chief = team.bots["Chief"]
    running(team, chief, from_agent=team.bots["Writer"], hop=collab_module.MAX_HOPS, chain="c1")
    with pytest.raises(ValueError, match="回報給使用者"):
        await team.collab.send(chief, "Scout", "再問一次", "request")


@pytest.mark.asyncio
async def test_stopping_a_bot_stops_the_work_it_handed_off(team):
    chief, writer, scout = team.bots["Chief"], team.bots["Writer"], team.bots["Scout"]
    running(team, chief)
    await team.collab.send(chief, "Writer", "寫", "request")
    [child] = queued(team, writer)
    # Writer passes part of it on to Scout.
    team.store.patch("tasks", child["id"], state="running"); team.runtime.active[writer] = child["id"]
    await team.collab.send(writer, "Scout", "查資料", "request")
    [grandchild] = queued(team, scout)
    team.runtime.active.pop(chief); team.store.patch("tasks", team.store.all("tasks")[0]["id"], state="terminal", verdict="answer")
    stopped = team.collab.stop_cascade(chief, [child["parent_task"]])
    await settle()
    assert {child["id"], grandchild["id"]} <= stopped
    assert team.store.get("tasks", grandchild["id"])["verdict"] == "cancelled"
    assert team.store.get("tasks", child["id"])["state"] in {"stopping", "terminal"}


def test_mentions_match_members_and_everyone():
    members = [{"id": "1", "name": "Writer"}, {"id": "2", "name": "Write"}, {"id": "3", "name": "小編"}]
    named, everyone = mentions("@Writer 寫稿，@小編 排版，@everyone 注意時程", members)
    assert [m["id"] for m in named] == ["1", "3"] and everyone


@pytest.mark.asyncio
async def test_group_chat_wakes_the_named_bot_or_everyone(team):
    chief, writer, scout = team.bots["Chief"], team.bots["Writer"], team.bots["Scout"]
    with pytest.raises(ValueError, match="2 到 6"): team.collab.create_room([chief])
    room = team.collab.create_room([chief, writer, scout])
    assert room["name"] == "Chief、Writer、Scout" and room["auto_name"]
    woke = await team.collab.post(room["id"], "@Writer 寫一段開場")
    assert woke["woke"] == ["Writer"] and len(queued(team, writer)) == 1 and not queued(team, chief)
    # A second post before Writer starts does not stack another turn; it reads both when it starts.
    await team.collab.post(room["id"], "@Writer 再補一句結尾")
    assert len(queued(team, writer)) == 1
    woke = await team.collab.post(room["id"], "大家早")
    assert set(woke["woke"]) == {"Chief", "Writer", "Scout"}
    # Group seats live in hidden threads, never the bot's own chat.
    [seat] = queued(team, writer)
    assert team.store.get("threads", seat["thread_id"])["room_id"] == room["id"]
    prompt = team.runtime._prompt(writer, seat["goal"], seat)
    assert "寫一段開場" in prompt and "再補一句結尾" in prompt and "大家早" in prompt and "你被點名了" in prompt


@pytest.mark.asyncio
async def test_group_answers_are_posted_and_mentions_hand_off(team):
    chief, writer, scout = team.bots["Chief"], team.bots["Writer"], team.bots["Scout"]
    room = team.collab.create_room([chief, writer, scout], "發表會")
    await team.collab.post(room["id"], "@Chief 規劃發表會")
    [turn] = queued(team, chief)
    team.store.patch("tasks", turn["id"], state="running"); team.runtime.active[chief] = turn["id"]
    team.runtime.finish(chief, turn["id"], "answer", "@Scout 先查三個場地，回報價格")
    await settle()
    posted = team.store.transcript(room["id"], room["id"])[-1]
    assert posted["speaker_name"] == "Chief" and posted["mentions"] == [scout]
    [handoff] = queued(team, scout)
    assert handoff["addressed"] and handoff["round"] == 1 and handoff["low_priority"]
    # Silence is not posted, and a hand-off chain ends after ROOM_ROUNDS.
    team.store.patch("tasks", handoff["id"], state="running"); team.runtime.active[scout] = handoff["id"]
    team.runtime.finish(scout, handoff["id"], "answer", "[silent]")
    await settle()
    assert team.store.transcript(room["id"], room["id"])[-1]["id"] == posted["id"]
    tired = uid()
    team.store.put("tasks", tired, {"id": tired, "agent_id": writer, "goal": "x", "state": "running", "room_id": room["id"], "round": collab_module.ROOM_ROUNDS})
    team.runtime.finish(writer, tired, "answer", "@Chief 換你")
    await settle()
    assert not queued(team, chief)


def test_room_transcripts_survive_the_orphan_sweep(team):
    room = team.collab.create_room([team.bots["Chief"], team.bots["Writer"]])
    team.store.message(room["id"], "user", "hello", thread=room["id"])
    team.store.purge_orphans()
    assert team.store.transcript(room["id"], room["id"])[0]["content"] == "hello"


def make_factory(team):
    def create(body):
        id = uid(); row = {"id": id, "created_at": __import__("server.store", fromlist=["now"]).now(), **body}
        team.store.put("agents", id, row); team.store.agent_directory(id); return row
    team.collab.create_agent = create


@pytest.mark.asyncio
async def test_a_bot_opens_a_group_chat_and_hands_the_first_step(team):
    chief, writer, scout = team.bots["Chief"], team.bots["Writer"], team.bots["Scout"]
    running(team, chief)
    started = team.collab.start_group(chief, ["Writer", "Scout"], "發表會", "@Scout 先找三個場地")
    await settle()
    room = team.store.get("rooms", started["group_id"])
    assert set(room["member_ids"]) == {chief, writer, scout} and started["woke"] == ["Scout"]
    opening = team.store.transcript(room["id"], room["id"])[0]
    assert opening["speaker_name"] == "Chief" and opening["mentions"] == [scout]
    assert len(queued(team, scout)) == 1 and not queued(team, writer)
    event = team.store.transcript(chief, chief)[-1]
    assert event["event"] == "group_created" and event["room_id"] == room["id"]
    # Posting later wakes only the members named; a bot outside the group cannot post into it.
    posted = team.collab.post_group(chief, "發表會", "@Writer 場地確定後寫邀請函")
    await settle()
    assert posted["woke"] == ["Writer"] and len(queued(team, writer)) == 1
    outsider = uid(); team.store.put("agents", outsider, {"id": outsider, "name": "Outsider"})
    with pytest.raises(ValueError, match="找不到你所在的群組"): team.collab.post_group(outsider, "發表會", "hi")


@pytest.mark.asyncio
async def test_helper_bots_are_rate_limited_and_announced(team):
    chief = team.bots["Chief"]
    make_factory(team)
    running(team, chief)
    made = team.collab.create_bot(chief, "Designer", "設計", "負責視覺")
    helper = team.store.get("agents", made["bot_id"])
    assert helper["created_by"] == chief and helper["avatar_color"].startswith("#") and helper["title"] == "設計"
    assert team.store.transcript(chief, chief)[-1]["event"] == "bot_created"
    with pytest.raises(ValueError, match="已經有叫"): team.collab.create_bot(chief, "Designer")
    team.collab.create_bot(chief, "Editor"); team.collab.create_bot(chief, "Tester")
    with pytest.raises(ValueError, match="今天已經建立"): team.collab.create_bot(chief, "Fourth")


def test_a_bot_updates_its_own_profile_within_bounds(team):
    chief = team.bots["Chief"]
    assert team.collab.update_profile(chief, title="總指揮", avatar_shape="sun")["updated"] == ["title", "avatar_shape"]
    assert team.store.get("agents", chief)["avatar_shape"] == "sun"
    with pytest.raises(ValueError, match="#RRGGBB"): team.collab.update_profile(chief, avatar_color="red")
    with pytest.raises(ValueError, match="沒有要更新"): team.collab.update_profile(chief)


@pytest.mark.asyncio
async def test_questions_from_unattended_work_expire(team):
    writer = team.bots["Writer"]
    task = running(team, writer, from_agent=team.bots["Chief"], intent="request", session_id="s")
    answered = []
    async def answer(rid, result): answered.append(result)
    team.runtime.clients[writer] = SimpleNamespace(alive=True, answer=answer)
    await team.runtime.human_request(writer, {"id": "q1", "method": "approval", "params": {"command": "rm -rf /tmp/x"}})
    question = team.store.get("tasks", task)["question"]
    assert question["expires_at"]
    await team.runtime.expire_question(writer, task)
    assert answered == [{"choice": "deny"}]
    assert team.store.get("tasks", task)["question"] is None
    assert team.store.transcript(writer, writer)[-1]["event"] == "question_expired"
    # A turn the user started never expires.
    mine = running(team, team.bots["Scout"], session_id="s2")
    team.runtime.clients[team.bots["Scout"]] = SimpleNamespace(alive=True, answer=answer)
    await team.runtime.human_request(team.bots["Scout"], {"id": "q2", "method": "approval", "params": {"command": "x"}})
    assert "expires_at" not in team.store.get("tasks", mine)["question"]


@pytest.mark.asyncio
async def test_approvals_offer_always_allow_only_when_hermes_does(team):
    scout = team.bots["Scout"]
    task = running(team, scout, session_id="s")
    answered = []
    async def answer(rid, result): answered.append(result)
    team.runtime.clients[scout] = SimpleNamespace(alive=True, answer=answer)
    await team.runtime.human_request(scout, {"id": "a1", "method": "approval", "params": {"command": "npm i", "choices": ["once", "session", "always", "deny"]}})
    assert team.store.get("tasks", task)["question"]["options"] == ["允許這次", "一律允許", "拒絕"]
    await team.runtime.answer(scout, "a1", choice="always")
    assert answered == [{"choice": "always"}]
    await team.runtime.human_request(scout, {"id": "a2", "method": "approval", "params": {"command": "rm -rf /", "choices": ["once", "deny"]}})
    assert team.store.get("tasks", task)["question"]["options"] == ["允許這次", "拒絕"]
