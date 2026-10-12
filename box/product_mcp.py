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
    """Manage this bot's scheduled tasks shown in the web UI. Actions list/create/update/delete/pause/resume/run/history. schedule: frequency once/daily/weekdays/weekly/monthly, timezone e.g. Asia/Taipei, time HH:MM, optional date YYYY-MM-DD, weekdays 1=Mon..7=Sun, day_of_month. Clarify missing timing first. Only this scheduler triggers jobs."""
    return await call("routines", body={"action": action, "title": title, "instructions": instructions, "schedule": schedule or {}, "id": id or None})


@mcp.tool()
async def other_bots() -> dict:
    """List other bots available for delegation. They each have independent computers."""
    return await call("bots")


@mcp.tool()
async def delegate_to_bot(bot_id: str, text: str, request_id: str) -> dict:
    """Delegate a clear task to another bot. Reuse the same request_id if retrying. Returns a task ID to check later; do not repeatedly poll."""
    return await call("send", bot_id=bot_id, text=text, request_id=request_id)


@mcp.tool()
async def delegated_task(task_id: str) -> dict:
    """Read a delegated task's state and actual result. Wait before checking again if still running."""
    return await call("task", task_id=task_id)


if __name__ == "__main__": mcp.run(transport="stdio")
