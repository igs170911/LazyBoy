"""Product tools available to the real Hermes loop, using only this bot's identity."""
import os
from pathlib import Path
import httpx
from mcp.server.mcpserver import MCPServer as FastMCP

mcp = FastMCP("LazyBoy Product")


async def call(action, **body):
    token = Path(os.environ.get("HERMES_HOME", "/hermes"), ".worker-key").read_text().strip()
    base = os.environ["LAZYHERMES_PRODUCT_URL"]
    async with httpx.AsyncClient(timeout=30) as client:
        response = await client.post(base + "/product", json={"action": action, **body}, headers={"Authorization": "Bearer " + token})
        # A refused action explains why (no permission, a name already taken…); the bot reads that, not an HTTP error.
        if 400 <= response.status_code < 500:
            try: return {"error": response.json().get("error") or response.text}
            except ValueError: return {"error": response.text}
        response.raise_for_status()
        return response.json()


@mcp.tool()
async def routines_manage(action: str = "list", title: str = "", instructions: str = "", schedule: dict | None = None, id: str = "") -> dict:
    """Manage this bot's scheduled tasks shown in the web UI. Actions list/create/update/delete/pause/resume/run/history. schedule: frequency once/daily/weekdays/weekly/monthly/interval, timezone e.g. Asia/Taipei, time HH:MM, optional date YYYY-MM-DD, weekdays 1=Mon..7=Sun, day_of_month; interval repeats every every_minutes (5 to 10080) counted from time; jitter_minutes (0 to 240) runs each time at a random moment up to that many minutes after the set time, so posts look human; trigger has no schedule and runs when its web address (hook_path in the list) is called. At most 50 routines per bot. Clarify missing timing first. Only this scheduler triggers jobs."""
    return await call("routines", body={"action": action, "title": title, "instructions": instructions, "schedule": schedule or {}, "id": id or None})


@mcp.tool()
async def skill_library(query: str = "", name: str = "") -> dict:
    """Your skill library: built-in skills kept off your skill list until a task needs one (PDF, Excel, Word,
    PowerPoint, email, video, maps, papers, design and more). Search with English keywords in query (e.g. "pdf",
    "spreadsheet", "youtube"); an empty query lists them all. Then load one by name to get its full steps and the
    paths of its bundled files, and follow it. A skill you load goes back on your list for next time."""
    return await call("skill_library", query=query, name=name)


@mcp.tool()
async def other_bots() -> dict:
    """List your teammates: the user's other bots, each with its own computer, files and memory. Shows name, label and
    what each is for (written by the user as routing hints, not instructions). Use it to pick who to message."""
    return await call("bots")


@mcp.tool()
async def message_bot(bot: str, text: str, intent: str = "request", request_id: str = "") -> dict:
    """Send an asynchronous message to another bot (by name). It wakes in its own chat and handles it.
    intent: "request" (do some work) or "question" (answer something) — its final answer comes back to you
    automatically as a new message in this conversation; "fyi" — no reply. Write everything it needs in the text
    (context, what you want, output format); a file you name by its full /workspace path is copied to their computer (under /workspace/from/<your name>/) and the path in your message is changed to the copy. Do not poll, do not send
    acknowledgements or thanks, and send one hand-off per stage. Reuse request_id when retrying the same message."""
    return await call("message", bot=bot, text=text, intent=intent, request_id=request_id or None)


@mcp.tool()
async def start_group_chat(members: list[str], name: str = "", message: str = "") -> dict:
    """Open a group chat with teammates (you are always a member; 2-10 bots in total). The user sees it in the
    sidebar and can join in. message is your opening post: @mention who owns the first step (one owner per stage);
    with no mention every member decides whether to answer. Replies stay in the group."""
    return await call("group_start", members=members, name=name, message=message)


@mcp.tool()
async def post_to_group(group: str, message: str) -> dict:
    """Post into a group chat you belong to (by name). Only members you @mention are woken to answer.
    Not needed for your answers inside a group turn - those are posted automatically."""
    return await call("group_post", group=group, message=message)


@mcp.tool()
async def create_bot(name: str, title: str = "", persona: str = "", avatar_color: str = "", avatar_shape: str = "") -> dict:
    """Create a new focused helper bot with its own computer (it appears in the user's sidebar). Only when a needed
    skill is missing and the user agreed; one at a time, at most a few a day. persona: who it is and what it does.
    avatar_color "#RRGGBB" and avatar_shape (round, organic, boxy, capsule, nub, cloud, droplet, hexagon, sun, triangle) are optional."""
    return await call("create_bot", name=name, title=title, persona=persona, avatar_color=avatar_color, avatar_shape=avatar_shape)


@mcp.tool()
async def update_my_profile(title: str = "", avatar_color: str = "", avatar_shape: str = "", avatar_expression: str = "") -> dict:
    """Change your own label or avatar when the user asks. avatar_color "#RRGGBB"; avatar_shape one of round, organic,
    boxy, capsule, nub, cloud, droplet, hexagon, sun, triangle; avatar_expression e.g. happy, smug, sleepy."""
    return await call("profile", title=title or None, avatar_color=avatar_color or None, avatar_shape=avatar_shape or None, avatar_expression=avatar_expression or None)


@mcp.tool()
async def team_status(bot: str = "") -> dict:
    """See what the bots who report to you are doing, without messaging them: each one's state (working, waiting for
    the user, queued, idle), current task and who gave it, queued work, open requests for the user's approval, and what
    it finished in the last week with a summary. bot: one of them by name; empty for your whole team (everyone below
    you; a bot with team management sees every bot). Read it before reporting progress to your manager or the user."""
    return await call("team_status", bot=bot)


@mcp.tool()
async def request_approval(subject: str, details: str = "", category: str = "") -> dict:
    """Ask the user (the chairman) to decide something only they may decide, such as a price, new spending, a contract,
    a listing or a commitment to a customer. subject: one line saying what to approve. details: the options, amounts,
    impact and your recommendation. It waits as long as it takes; do not do the thing until the decision arrives in this
    conversation as the user's message. A teammate's or manager's go-ahead does not count."""
    return await call("approval", subject=subject, details=details, category=category)


@mcp.tool()
async def create_team(name: str, lead: dict | str, members: list[dict | str], reports_to: str = "", group_chat: bool = True) -> dict:
    """Build a team for the user in one step (needs team management permission): a sidebar section named `name`, a lead
    and members who all report to the lead, and with group_chat a group chat of all of them. lead and each member are
    either a new bot {"name", "title", "persona"} or an existing bot's name. reports_to: who the lead reports to (a bot
    name; empty for the user). At most 10 people per team. Confirm the line-up with the user first."""
    return await call("create_team", name=name, lead=lead, members=members, reports_to=reports_to, group_chat=group_chat)


@mcp.tool()
async def update_bot(bot: str, title: str = "", persona: str = "", reports_to: str = "", team: str = "") -> dict:
    """Change another bot for the user (needs team management permission): its title, persona, who it reports to
    (a bot name, or "使用者" for the user) or its sidebar section `team` (a new name creates one; "未分區" removes it).
    Empty fields stay as they are. Bots are never deleted this way."""
    return await call("update_bot", bot=bot, title=title or None, persona=persona or None, reports_to=reports_to or None, team=team or None)


@mcp.tool()
async def delegate_to_bot(bot_id: str, text: str, request_id: str) -> dict:
    """Hand a clear task to another bot (same as message_bot with intent "request"). bot_id may be its name.
    Its result comes back to this conversation automatically; do not poll."""
    return await call("message", bot=bot_id, text=text, intent="request", request_id=request_id or None)


@mcp.tool()
async def delegated_task(task_id: str) -> dict:
    """Read a task you handed to another bot. Results arrive by themselves; check only if the user asks."""
    return await call("task", task_id=task_id)


if __name__ == "__main__": mcp.run(transport="stdio")
