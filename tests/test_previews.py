"""Office previews and archive listings: converted on the bot's computer, never trusted as LazyBoy's own pages."""
import importlib
import io
import zipfile
from types import SimpleNamespace

import httpx
import pytest

from server import previews as previews_module
from server.previews import Previews, archive_entries
from server.store import Store, uid


@pytest.fixture
def setup(tmp_path, monkeypatch):
    store = Store(tmp_path); bot = uid()
    workspace = store.agent_directory(bot) / "workspace"; workspace.mkdir(parents=True, exist_ok=True)
    sheet = workspace / "budget.xlsx"; sheet.write_bytes(b"xlsx")
    calls = []
    async def docker(*args, **kwargs):
        # LibreOffice on the bot's computer writes the page into the --outdir it was given.
        calls.append(args)
        outdir = args[args.index("--outdir") + 1].removeprefix("/home/box/")
        out = store.agent_directory(bot) / "home" / outdir / "budget.html"
        out.parent.mkdir(parents=True, exist_ok=True); out.write_text("<table><tr><td>1</td></tr></table>")
        return 0, "", ""
    monkeypatch.setattr(previews_module.computers_module, "docker", docker)
    monkeypatch.setattr(previews_module.computers_module, "user_flag", lambda: "1000:1000")
    async def status(id, refresh=False): return {"ready": True}
    computers = SimpleNamespace(status=status, name=lambda id: "lazyhermes-bot-" + id)
    return SimpleNamespace(store=store, bot=bot, sheet=sheet, calls=calls, previews=Previews(store, computers))


async def test_a_converted_page_is_cached_and_reused(setup):
    out, kind = await setup.previews.office(setup.bot, setup.sheet)
    assert kind == "html" and "<table>" in out.read_text()
    await setup.previews.office(setup.bot, setup.sheet)
    assert len(setup.calls) == 1


async def test_a_link_the_bot_left_in_the_cache_is_never_served(setup, tmp_path):
    secret = tmp_path / "host-secret.txt"; secret.write_text("HOST_SECRET")
    folder, out = setup.previews.cached(setup.bot, setup.sheet, "html")
    folder.mkdir(parents=True); out.symlink_to(secret)
    served, _ = await setup.previews.office(setup.bot, setup.sheet)
    assert not served.is_symlink() and "HOST_SECRET" not in served.read_text() and len(setup.calls) == 1


async def test_a_cache_folder_pointing_off_the_bots_computer_is_refused(setup, tmp_path):
    elsewhere = tmp_path / "elsewhere"; elsewhere.mkdir()
    cache = setup.store.agent_directory(setup.bot) / "home" / ".cache"; cache.mkdir(parents=True, exist_ok=True)
    (cache / "lazyboy-preview").symlink_to(elsewhere)
    with pytest.raises(ValueError, match="不在這個 Bot"): await setup.previews.office(setup.bot, setup.sheet)
    assert not setup.calls and not any(elsewhere.iterdir())


def test_archives_are_listed_without_extracting(tmp_path):
    data = io.BytesIO()
    with zipfile.ZipFile(data, "w") as archive:
        archive.writestr("report/summary.md", "hi"); archive.writestr("report/", "")
    path = tmp_path / "files.zip"; path.write_bytes(data.getvalue())
    listing = archive_entries(path)
    assert listing["total"] == 2 and {"name": "report/summary.md", "bytes": 2, "dir": False} in listing["entries"]
    (tmp_path / "broken.zip").write_bytes(b"PK\x03\x04 not really")
    with pytest.raises(ValueError): archive_entries(tmp_path / "broken.zip")


async def test_a_bot_page_opened_directly_cannot_run_as_lazyboy(setup, monkeypatch):
    main = importlib.import_module("server.main")
    monkeypatch.setattr(main, "store", setup.store)
    monkeypatch.setattr(main, "previews", setup.previews)
    monkeypatch.setattr(main.runtime, "agent", lambda id: {"id": id})
    async with httpx.AsyncClient(transport=httpx.ASGITransport(app=main.app), base_url="http://localhost:8790") as client:
        page = await client.get(f"/api/agents/{setup.bot}/files/office", params={"path": "/workspace/budget.xlsx"})
    assert page.status_code == 200 and page.headers["x-preview-kind"] == "html"
    assert page.headers["content-security-policy"].startswith("sandbox") and "script" not in page.headers["content-security-policy"]
