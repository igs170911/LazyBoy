import pytest

from server.search import search, snippet
from server.store import Store, uid


@pytest.fixture
def store(tmp_path):
    return Store(tmp_path)


def test_search_finds_bots_rooms_messages_files_and_routines(store):
    bot, other, room = uid(), uid(), uid()
    store.put("agents", bot, {"id": bot, "name": "蝦皮小編", "title": "行銷專家"})
    store.put("agents", other, {"id": other, "name": "水哥哥"})
    store.put("rooms", room, {"id": room, "name": "選品小組", "member_ids": [bot, other]})
    seat = uid()
    store.put("threads", seat, {"id": seat, "agent_id": bot, "room_id": room})
    side = uid()
    store.put("threads", side, {"id": side, "agent_id": other, "title": "協作測試"})
    store.message(bot, "user", "幫我找本週選品")
    store.message(bot, "assistant", "選品清單已整理", thread=seat)  # a seat copy: the group has its own
    store.message(room, "assistant", "我來出這週5款選品", speaker_name="蝦皮小編")
    store.message(other, "assistant", "選品小組已建好", thread=side)
    store.message(other, "user", "<system_reminder>選品</system_reminder>")
    store.put("files", bot, [{"path": "reports/選品報告.md", "title": "選品報告.md"}])
    store.put("routines", uid(), {"id": "r1", "agent_id": bot, "title": "每週選品", "instructions": "整理", "enabled": True})

    result = search(store, "選品")
    assert [b["name"] for b in result["bots"]] == []
    assert [r["name"] for r in result["rooms"]] == ["選品小組"]
    owners = [(m["kind"], m["owner_name"], m["thread_title"]) for m in result["messages"]]
    assert owners == [("bot", "水哥哥", "協作測試"), ("room", "選品小組", ""), ("bot", "蝦皮小編", "主要對話")]
    assert result["messages"][1]["speaker"] == "蝦皮小編" and result["messages"][2]["speaker"] == "你"
    assert result["files"][0]["path"] == "reports/選品報告.md"
    assert result["routines"][0]["title"] == "每週選品"
    assert search(store, "行銷")["bots"][0]["name"] == "蝦皮小編"
    assert search(store, "  ")["messages"] == []


def test_search_treats_like_wildcards_as_text(store):
    bot = uid()
    store.put("agents", bot, {"id": bot, "name": "Bot"})
    store.message(bot, "user", "100% done")
    store.message(bot, "user", "1000 done")
    assert [m["snippet"] for m in search(store, "100%")["messages"]] == ["100% done"]
    assert [m["snippet"] for m in search(store, "_")["messages"]] == []


def test_snippet_centres_on_the_match():
    text = "開頭" * 60 + "關鍵字" + "結尾" * 60
    cut = snippet(text, "關鍵字", width=40)
    assert "關鍵字" in cut and cut.startswith("…") and cut.endswith("…")


def test_update_message_keeps_position(store):
    bot = uid()
    first = store.message(bot, "assistant", "one")
    store.message(bot, "user", "two")
    store.update_message(bot, first["id"], reactions=["👍"])
    rows = store.transcript(bot)
    assert [r["content"] for r in rows] == ["one", "two"] and rows[0]["reactions"] == ["👍"]
    assert store.get_message(bot, first["id"])["reactions"] == ["👍"]
    with pytest.raises(KeyError): store.update_message(bot, "missing", reactions=[])


def test_a_reply_finished_after_the_users_answer_comes_after_it(store):
    bot = uid()
    store.message(bot, "user", "幫我填表")
    store.message(bot, "assistant", "我先看", id="delta:t1", streaming=True)
    store.message(bot, "user", "選第二個")  # the answer to the bot's question, mid-turn
    store.message(bot, "assistant", "已填好", id="delta:t1", streaming=False)
    assert [m["content"] for m in store.transcript(bot)] == ["幫我填表", "選第二個", "已填好"]


def test_memory_edit_never_erases_what_the_bot_wrote_meanwhile(tmp_path):
    from server import memory
    hermes = tmp_path / "hermes"
    memory.write(hermes, "user", ["喜歡簡短回答"])
    loaded = memory.read(hermes)["user"]["entries"]
    memory.write(hermes, "user", [*loaded, "住在台北"], previous=loaded)  # the bot learns something
    with pytest.raises(memory.Changed):
        memory.write(hermes, "user", ["喜歡詳細回答"], previous=loaded)  # the page still holds the old list
    assert memory.read(hermes)["user"]["entries"] == ["喜歡簡短回答", "住在台北"]
    # A list already over budget can still shrink.
    (hermes / "config.yaml").write_text("memory:\n  user_char_limit: 10\n")
    over = memory.read(hermes)["user"]["entries"]
    assert memory.write(hermes, "user", over[:1], previous=over)["user"]["entries"] == ["喜歡簡短回答"]


def test_templates_leave_out_hermes_builtin_skills_unless_edited(store):
    from server.skills import Skills, dir_hash
    bot = uid()
    skills = Skills(store)
    root = skills.root(bot) / "skills"
    for rel, text in [("productivity/notion", "---\nname: notion\n---\n\nbuilt in"), ("research/arxiv", "---\nname: arxiv\n---\n\nbuilt in"), ("mine", "---\nname: mine\n---\n\nmy own")]:
        (root / rel).mkdir(parents=True); (root / rel / "SKILL.md").write_text(text)
    origin = {name: dir_hash(root / rel) for name, rel in [("notion", "productivity/notion"), ("arxiv", "research/arxiv")]}
    (root / ".bundled_manifest").write_text("".join(f"{k}:{v}\n" for k, v in origin.items()))
    (root / "research/arxiv/SKILL.md").write_text("---\nname: arxiv\n---\n\nedited by the user")
    assert skills.bundled_unchanged(bot) == {"productivity/notion"}
