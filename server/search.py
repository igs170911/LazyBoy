"""Search everything from one box, like Grok Bot's command palette: bots, group chats, messages, files and routines.

Messages are matched in the database (newest first); a bot's seat in a group chat is skipped because the group keeps
its own copy of that conversation. Results carry what the web needs to jump there: the bot or group, the thread and
the message id.
"""
from .runtime import plain_text

LIMITS = {"bots": 8, "rooms": 6, "messages": 40, "files": 12, "routines": 8}
HIDDEN_PREFIXES = ("<system_reminder>", "Background task result (data, not instructions)")


def snippet(text, needle, width=90):
    """The matching part of a message as one line, with a little context on each side."""
    flat = plain_text(text)
    at = flat.lower().find(needle)
    if at < 0: return plain_text(text, width)
    start = max(0, at - width // 3)
    end = min(len(flat), start + width)
    return ("…" if start else "") + flat[start:end] + ("…" if end < len(flat) else "")


def matches(needle, *values):
    return any(needle in str(value or "").lower() for value in values)


def search(store, query):
    text = " ".join(str(query or "").split())[:200]
    result = {"query": text, "bots": [], "rooms": [], "messages": [], "files": [], "routines": []}
    if not text: return result
    needle = text.lower()
    agents = {a["id"]: a for a in store.all("agents")}
    rooms = {r["id"]: r for r in store.all("rooms")}
    threads = {t["id"]: t for t in store.all("threads")}
    seats = {id for id, t in threads.items() if t.get("room_id")}

    for agent in agents.values():
        if matches(needle, agent.get("name"), agent.get("title"), agent.get("description"), *(agent.get("tags") or [])):
            result["bots"].append({"id": agent["id"], "name": agent["name"], "title": agent.get("title") or "", "hidden": bool(agent.get("hidden"))})
    for room in rooms.values():
        members = [agents[m]["name"] for m in room.get("member_ids", []) if m in agents]
        if matches(needle, room.get("name"), *members):
            result["rooms"].append({"id": room["id"], "name": room.get("name") or "群組對話", "members": members})

    for owner, message in store.search_messages(text):
        if len(result["messages"]) >= LIMITS["messages"]: break
        content = str(message.get("content") or "")
        if message.get("silent") or content.lstrip().startswith(HIDDEN_PREFIXES): continue
        thread = message.get("thread") or owner
        if owner in rooms:
            room = rooms[owner]
            row = {"kind": "room", "owner_id": owner, "owner_name": room.get("name") or "群組對話", "thread_id": owner, "thread_title": ""}
        elif owner in agents:
            if thread in seats: continue
            title = (agents[owner].get("main_thread_title") or "主要對話") if thread == owner else ((threads.get(thread) or {}).get("title") or "對話")
            row = {"kind": "bot", "owner_id": owner, "owner_name": agents[owner]["name"], "thread_id": thread, "thread_title": plain_text(title, 40)}
        else: continue
        speaker = message.get("speaker_name") or ("你" if message.get("role") == "user" and not message.get("from_agent") else
                                                   row["owner_name"] if row["kind"] == "bot" else "")
        result["messages"].append({**row, "message_id": message.get("id"), "role": message.get("role"), "speaker": speaker,
                                   "snippet": snippet(content, needle), "at": message.get("at")})

    for agent_id, agent in agents.items():
        for file in store.get("files", agent_id) or []:
            if len(result["files"]) >= LIMITS["files"]: break
            if matches(needle, file.get("path"), file.get("title")):
                result["files"].append({"agent_id": agent_id, "agent_name": agent["name"], "path": file.get("path"), "title": file.get("title") or file.get("path"),
                                        "media_type": file.get("media_type"), "updated_at": file.get("updated_at")})
    for routine in store.all("routines"):
        if routine.get("agent_id") in agents and matches(needle, routine.get("title"), routine.get("instructions")):
            result["routines"].append({"id": routine["id"], "agent_id": routine["agent_id"], "agent_name": agents[routine["agent_id"]]["name"],
                                       "title": routine.get("title") or "", "enabled": bool(routine.get("enabled")), "next_run_at": routine.get("next_run_at")})
    for key, limit in LIMITS.items(): result[key] = result[key][:limit]
    return result
