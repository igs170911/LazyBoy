"""The skill library: Hermes' built-in skills a bot is not using wait outside the list sent with every request."""
import json
from datetime import datetime, timedelta, timezone

import pytest
import yaml

from server.skills import Skills, dir_hash
from server.store import Store, uid


@pytest.fixture
def store(tmp_path):
    return Store(tmp_path)


def bot_with_skills(store):
    """A bot with two untouched Hermes skills, one Hermes skill it uses, and one skill it learned."""
    bot = uid(); skills = Skills(store)
    root = skills.root(bot) / "skills"
    files = {"productivity/pdf": "---\nname: pdf\ndescription: PDF files - create, read, merge\nplatforms: [linux]\n---\n\nUse pypdf.",
             "media/youtube-content": "---\nname: youtube-content\ndescription: YouTube transcripts to summaries\n---\n\nFetch the transcript.",
             "research/deep-dive": "---\nname: deep-dive\ndescription: Compare many sources\n---\n\nSplit the question.",
             "threads-patrol": "---\nname: threads-patrol\ndescription: Reply on Threads\n---\n\nUse the reply row."}
    for rel, text in files.items():
        (root / rel).mkdir(parents=True); (root / rel / "SKILL.md").write_text(text)
    (root / "productivity/pdf/scripts").mkdir(); (root / "productivity/pdf/scripts/merge.py").write_text("print('merge')")
    bundled = {"pdf": "productivity/pdf", "youtube-content": "media/youtube-content", "deep-dive": "research/deep-dive"}
    (root / ".bundled_manifest").write_text("".join(f"{name}:{dir_hash(root / rel)}\n" for name, rel in bundled.items()))
    used = (datetime.now(timezone.utc) - timedelta(days=2)).isoformat()
    (root / ".usage.json").write_text(json.dumps({"deep-dive": {"last_used_at": used}, "threads-patrol": {"last_used_at": used}}))
    return bot, skills


def prepared(skills, bot, cfg=None):
    cfg = cfg or skills.config(bot)
    library = skills.apply_library(bot, cfg)
    skills.write_config(bot, cfg)
    return library, cfg


def test_unused_builtin_skills_leave_the_list_and_the_bots_own_stay(store):
    bot, skills = bot_with_skills(store)
    library, cfg = prepared(skills, bot)
    assert library == {"pdf", "youtube-content"}
    # Hermes lists only skills that are not disabled; the library is how they leave the per-request list.
    assert set(cfg["skills"]["disabled"]) == {"pdf", "youtube-content"}
    places = {row["name"]: row["place"] for row in skills.list(bot)}
    assert places == {"pdf": "library", "youtube-content": "library", "deep-dive": "list", "threads-patrol": "list"}
    assert all(row["enabled"] for row in skills.list(bot))


def test_the_bot_searches_and_loads_a_library_skill_which_then_returns_to_its_list(store):
    bot, skills = bot_with_skills(store)
    prepared(skills, bot)
    found = skills.library_search(bot, "pdf merge")
    assert [s["name"] for s in found["skills"]] == ["pdf"] and found["library_size"] == 2
    assert {s["name"] for s in skills.library_search(bot)["skills"]} == {"pdf", "youtube-content"}
    assert skills.library_search(bot, "spreadsheet")["skills"] == []
    opened = skills.library_open(bot, "pdf")
    assert opened["instructions"] == "Use pypdf." and opened["files"] == ["/hermes/skills/productivity/pdf/scripts/merge.py"]
    library, cfg = prepared(skills, bot)
    assert library == {"youtube-content"} and cfg["skills"]["disabled"] == ["youtube-content"]


def test_what_the_user_switches_off_or_places_is_kept(store):
    bot, skills = bot_with_skills(store)
    prepared(skills, bot)
    skills.manage(bot, {"action": "toggle", "id": "media/youtube-content", "enabled": False})
    skills.manage(bot, {"action": "place", "id": "research/deep-dive", "place": "library"})
    skills.manage(bot, {"action": "place", "id": "productivity/pdf", "place": "list"})
    library, cfg = prepared(skills, bot)
    assert library == {"deep-dive"}
    assert set(cfg["skills"]["disabled"]) == {"youtube-content", "deep-dive"}
    places = {row["name"]: row["place"] for row in skills.list(bot)}
    assert places == {"pdf": "list", "youtube-content": "off", "deep-dive": "library", "threads-patrol": "list"}
    with pytest.raises(ValueError): skills.library_open(bot, "youtube-content")
    # Switching it back on puts it on the list for good.
    skills.manage(bot, {"action": "toggle", "id": "media/youtube-content", "enabled": True})
    assert prepared(skills, bot)[0] == {"deep-dive"}


def test_switching_a_builtin_skill_off_and_on_leaves_its_file_untouched(store):
    bot, skills = bot_with_skills(store)
    path = skills.root(bot) / "skills/productivity/pdf/SKILL.md"
    before = path.read_text()
    skills.manage(bot, {"action": "toggle", "id": "productivity/pdf", "enabled": False})
    skills.manage(bot, {"action": "toggle", "id": "productivity/pdf", "enabled": True})
    assert path.read_text() == before and "productivity/pdf" in skills.bundled_unchanged(bot)
    # Editing keeps the frontmatter Hermes needs.
    skills.manage(bot, {"action": "save", "id": "productivity/pdf", "name": "pdf", "description": "PDF tools", "instructions": "Use qpdf."})
    meta = yaml.safe_load(path.read_text().split("---")[1])
    assert meta == {"name": "pdf", "description": "PDF tools", "platforms": ["linux"]}


def test_a_deleted_skill_leaves_no_name_behind(store):
    bot, skills = bot_with_skills(store)
    skills.manage(bot, {"action": "place", "id": "threads-patrol", "place": "library"})
    skills.manage(bot, {"action": "toggle", "id": "media/youtube-content", "enabled": False})
    skills.manage(bot, {"action": "delete", "id": "threads-patrol"})
    skills.manage(bot, {"action": "delete", "id": "media/youtube-content"})
    cfg = skills.config(bot)
    assert "threads-patrol" not in skills.library_names(bot) and "threads-patrol" not in cfg["product_skill_library"]
    assert not {"threads-patrol", "youtube-content"} & set(cfg["skills"]["disabled"])
    assert not {"threads-patrol", "youtube-content"} & set(store.get("skill_library", bot, {}).get("places") or {})
