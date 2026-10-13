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
        response.raise_for_status()
        return response.json()


@mcp.tool()
async def routines_manage(action: str = "list", title: str = "", instructions: str = "", schedule: dict | None = None, id: str = "") -> dict:
    """Manage this bot's scheduled tasks shown in the web UI. Actions list/create/update/delete/pause/resume/run/history. schedule: frequency once/daily/weekdays/weekly/monthly/interval, timezone e.g. Asia/Taipei, time HH:MM, optional date YYYY-MM-DD, weekdays 1=Mon..7=Sun, day_of_month; interval repeats every every_minutes (5 to 10080) counted from time; trigger has no schedule and runs when its web address (hook_path in the list) is called. At most 50 routines per bot. Clarify missing timing first. Only this scheduler triggers jobs."""
    return await call("routines", body={"action": action, "title": title, "instructions": instructions, "schedule": schedule or {}, "id": id or None})


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
    (context, what you want, output format); files are on your own computer, not theirs. Do not poll, do not send
    acknowledgements or thanks, and send one hand-off per stage. Reuse request_id when retrying the same message."""
    return await call("message", bot=bot, text=text, intent=intent, request_id=request_id or None)


@mcp.tool()
async def start_group_chat(members: list[str], name: str = "", message: str = "") -> dict:
    """Open a group chat with teammates (you are always a member; 2-6 bots in total). The user sees it in the
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
async def delegate_to_bot(bot_id: str, text: str, request_id: str) -> dict:
    """Hand a clear task to another bot (same as message_bot with intent "request"). bot_id may be its name.
    Its result comes back to this conversation automatically; do not poll."""
    return await call("message", bot=bot_id, text=text, intent="request", request_id=request_id or None)


@mcp.tool()
async def delegated_task(task_id: str) -> dict:
    """Read a task you handed to another bot. Results arrive by themselves; check only if the user asks."""
    return await call("task", task_id=task_id)


if __name__ == "__main__": mcp.run(transport="stdio")
