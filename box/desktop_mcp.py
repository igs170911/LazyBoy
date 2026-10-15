"""The bot's own X11 desktop, exposed through Hermes' native MCP integration.

DOM automation remains Hermes browser_exec over the same Chromium CDP session.
Desktop tools return actual PNG image content when requested, plus AT-SPI text.
"""
import asyncio
import json
import os
from pathlib import Path
import tempfile

from mcp.server.mcpserver import MCPServer as FastMCP, Image

mcp = FastMCP("Bot Computer")
observed = {}
desktop_lock = asyncio.Lock()


def screen_size():
    """The desktop's size as start-desktop.sh set it (LAZYBOY_SCREEN_GEOMETRY, e.g. 1280x720)."""
    try:
        width, height = os.environ.get("LAZYBOY_SCREEN_GEOMETRY", "1280x720").lower().split("x")
        return int(width), int(height)
    except ValueError:
        return 1280, 720


async def command(*args: str, timeout: float = 12) -> str:
    p = await asyncio.create_subprocess_exec(*args, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
    try:
        out, err = await asyncio.wait_for(p.communicate(), timeout)
    except TimeoutError:
        p.kill()
        await p.wait()
        raise ValueError("Desktop operation timed out; inspect current state before retrying")
    if p.returncode:
        raise ValueError(err.decode(errors="replace")[-1800:] or "Desktop operation failed")
    return out.decode(errors="replace")


async def snapshot():
    data = json.loads(await command("/usr/bin/python3", "/usr/local/bin/box-a11y", "--json", "--limit", "180", "--budget", "3"))
    return data.get("elements", [])


def fingerprint(row):
    return tuple(row.get(key) for key in ("window", "role", "name", "cx", "cy", "w", "h"))


async def observation(image=False):
    rows = await snapshot()
    observed.clear()
    observed.update({row["id"]:fingerprint(row) for row in rows})
    text = "\n".join(f'[{row["id"]}] {row["role"]} {json.dumps(row.get("name", ""), ensure_ascii=False)}'
                     f' @ ({row["cx"]},{row["cy"]}) {row["w"]}x{row["h"]}'
                     + (" value=" + json.dumps(row["value"], ensure_ascii=False) if row.get("value") else "")
                     for row in rows)
    result = [text or "No accessible elements. Request an image to inspect visual UI."]
    if image:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp, "screen.png")
            await command("import", "-window", "root", "-resize", "1280x1280>", str(path))
            result.append(Image(data=path.read_bytes(), format="png"))
    return result


@mcp.tool()
async def desktop_observe(image: bool = False) -> list:
    """Observe the bot's own desktop. Prefer element IDs. Set image=true only for visual UI without accessible elements."""
    async with desktop_lock:
        return await observation(image)


@mcp.tool()
async def desktop_act(action: str, element: int | None = None, x: int | None = None,
                      y: int | None = None, text: str = "", key: str = "", amount: int = 3) -> str:
    """Act on the bot's own desktop and return a fresh observation. Actions: click, type, key, scroll. Use observed element IDs; coordinates only from an actual screenshot. Website work should prefer browser_exec."""
    async with desktop_lock:
        return await act(action, element, x, y, text, key, amount)


async def act(action, element, x, y, text, key, amount):
    if action == "click":
        if element is not None:
            target = observed.get(element)
            if target is None:
                raise ValueError("Observe the desktop before using an element ID")
            rows = await snapshot()
            hit = next((row for row in rows if fingerprint(row) == target), None)
            if hit is None:
                raise ValueError("Element no longer visible; observe again")
            x, y = hit.get("cx"), hit.get("cy")
        width, height = screen_size()
        if x is None or y is None or not (0 <= x < width and 0 <= y < height):
            raise ValueError("Valid observed element or in-screen coordinates required")
        await command("xdotool", "mousemove", str(x), str(y), "click", "1")
    elif action == "type":
        if len(text) > 20000:
            raise ValueError("Text exceeds 20000 characters")
        # xdotool needs a few ms per character beyond --delay; 20000 characters
        # must not be killed halfway through by the fixed command timeout.
        await command("xdotool", "type", "--clearmodifiers", "--delay", "1", "--", text, timeout=12 + len(text) / 200)
    elif action == "key":
        if not key or len(key) > 100:
            raise ValueError("A key chord is required, e.g. ctrl+l or Return")
        await command("xdotool", "key", "--clearmodifiers", key)
    elif action == "scroll":
        await command("xdotool", "click", "--repeat", str(min(max(abs(amount), 1), 15)), "5" if amount >= 0 else "4")
    else:
        raise ValueError("Unsupported action")
    await asyncio.sleep(.15)
    return (await observation())[0]


if __name__ == "__main__":
    os.environ.setdefault("DISPLAY", ":1")
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS"):
        for line in Path("/tmp/lazyboy/dbus.env").read_text().splitlines():
            name, _, value = line.partition("=")
            if name == "DBUS_SESSION_BUS_ADDRESS": os.environ[name] = value
    mcp.run(transport="stdio")
