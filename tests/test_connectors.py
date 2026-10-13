"""Connect apps: the catalog, one-click connect, OAuth sign-in on a bot's computer, and one shared sign-in for all bots."""
from types import SimpleNamespace

import pytest
import yaml

from server import connectors as connectors_module
from server.computers import Computers
from server.config import Settings
from server.connectors import Connectors, catalog
from server.runtime import Runtime
from server.store import Store, uid


class FakeClient:
    alive = True

    def __init__(self, replies):
        self.replies, self.calls = replies, []

    async def call(self, method, params=None, timeout=None):
        self.calls.append((method, params))
        reply = self.replies.get(method, {})
        return reply(params) if callable(reply) else reply


@pytest.fixture
def apps(tmp_path, monkeypatch):
    store = Store(tmp_path)
    computers = Computers(store, Settings(store))
    runtime = Runtime(store, Settings(store), computers)
    runtime.closed = True
    bot = uid(); store.put("agents", bot, {"id": bot, "name": "Bot"})
    client = FakeClient({"mcp.servers.test": {"ok": True, "tools": [{"name": "search", "description": "Search"}]}})
    runtime.clients[bot] = client
    async def connected(id): return runtime.clients[id]
    runtime.client = connected
    docker_calls = []
    async def docker(*args, **kwargs):
        docker_calls.append(args)
        return 0, "200", ""
    monkeypatch.setattr(connectors_module.computers_module, "docker", docker)
    yield SimpleNamespace(store=store, computers=computers, runtime=runtime, bot=bot, client=client, docker=docker_calls,
                          connectors=Connectors(store, runtime, computers))
    store.conn.close()


def test_catalog_lists_hosted_apps_with_names_and_leaves_out_ones_needing_a_developer_app():
    items = catalog()
    assert list(items)[0] == "notion" and items["notion"]["auth"] == "oauth" and items["notion"]["title"] == "Notion"
    assert items["context7"]["auth"] == "none" and items["github"]["auth"] == "token"
    assert not {"asana", "n8n-official", "unreal-engine"} & set(items)
    assert all(item["url"].startswith("https://") and item["category"] in connectors_module.CATEGORIES for item in items.values())
    assert items["calendly"]["exclude"]  # the manifest's default_excluded tools stay off


@pytest.mark.asyncio
async def test_an_app_without_sign_in_connects_for_every_bot(apps):
    view = await apps.connectors.connect("context7")
    assert view["state"] == "connected" and view["tools"][0]["name"] == "search"
    entry = apps.store.get("mcp", "shared")["context7"]
    assert entry == {"url": "https://mcp.context7.com/mcp", "catalog": "context7", "enabled": True}
    config = yaml.safe_load((apps.store.agent_directory(apps.bot) / "hermes" / "config.yaml").read_text())
    assert config["mcp_servers"]["context7"] == {"url": "https://mcp.context7.com/mcp"}  # the product's marker stays out


@pytest.mark.asyncio
async def test_oauth_sign_in_opens_on_the_bots_computer_and_finishes_when_approved(apps):
    apps.client.replies["mcp.servers.oauth.start"] = {"ok": True, "session_id": "s1", "auth_url": "https://notion.example/authorize?x=1"}
    apps.client.replies["mcp.servers.oauth.poll"] = {"status": "pending"}
    view = await apps.connectors.connect("notion", apps.bot)
    assert view["state"] == "signing_in" and view["signin"] == {"agent": apps.bot, "auth_url": "https://notion.example/authorize?x=1"}
    assert ("mcp.servers.oauth.start", {"name": "notion"}) in apps.client.calls
    assert any(call[-2:] == ("/usr/local/bin/box-chrome", "https://notion.example/authorize?x=1") for call in apps.docker)
    assert (await apps.connectors.poll_app("notion"))["state"] == "signing_in"

    # Hermes saved the tokens in the folder every bot mounts.
    tokens = apps.store.directory / "mcp-tokens"; tokens.mkdir(exist_ok=True)
    (tokens / "notion.json").write_text("{}")
    apps.client.replies["mcp.servers.oauth.poll"] = {"status": "approved", "tools": [{"name": "notion-search", "description": ""}]}
    view = await apps.connectors.poll_app("notion")
    assert view["state"] == "connected" and view["signin"] is None and view["tools"][0]["name"] == "notion-search"
    assert any(method == "reload.mcp" for method, _ in apps.client.calls)


@pytest.mark.asyncio
async def test_a_pasted_return_address_is_relayed_to_the_receiver_in_the_computer(apps):
    apps.client.replies["mcp.servers.oauth.start"] = {"ok": True, "session_id": "s1", "auth_url": "https://linear.example/a"}
    apps.client.replies["mcp.servers.oauth.poll"] = {"status": "approved", "tools": [{"name": "issues"}]}
    await apps.connectors.connect("linear", apps.bot)
    with pytest.raises(ValueError, match="完整網址"):
        await apps.connectors.relay_app("linear", "https://evil.example/callback?code=1")
    view = await apps.connectors.relay_app("linear", "http://127.0.0.1:45123/callback?code=abc&state=xyz")
    assert view["state"] == "needs_signin" or view["state"] == "connected"
    curl = next(call for call in apps.docker if "curl" in call)
    assert curl[-1] == "http://127.0.0.1:45123/callback?code=abc&state=xyz" and curl[1] == apps.computers.name(apps.bot)


@pytest.mark.asyncio
async def test_disconnect_removes_the_app_and_its_sign_in(apps):
    apps.client.replies["mcp.servers.oauth.start"] = {"ok": True, "session_id": "s1", "auth_url": "https://x.example/a"}
    await apps.connectors.connect("figma", apps.bot)
    tokens = apps.store.directory / "mcp-tokens"
    for suffix in (".json", ".client.json", ".meta.json"): (tokens / f"figma{suffix}").write_text("{}")
    view = await apps.connectors.disconnect("figma")
    assert view["state"] == "available" and "figma" not in apps.store.get("mcp", "shared", {})
    assert not list(tokens.glob("figma*"))
    assert ("mcp.servers.oauth.cancel", {"name": "figma", "session_id": "s1"}) in apps.client.calls


@pytest.mark.asyncio
async def test_a_custom_connector_with_the_same_name_is_not_overwritten(apps):
    apps.store.put("mcp", "shared", {"notion": {"url": "https://my.example/mcp"}})
    with pytest.raises(ValueError, match="自訂串接"):
        await apps.connectors.connect("notion")


@pytest.mark.asyncio
async def test_a_token_app_needs_its_token(apps):
    with pytest.raises(ValueError, match="Token"):
        await apps.connectors.connect("github")
    view = await apps.connectors.connect("github", token="ghp_example")
    assert view["state"] == "connected"
    assert apps.store.get("mcp", "shared")["github"]["headers"] == {"Authorization": "Bearer ghp_example"}


def test_sign_ins_saved_in_a_bots_own_home_move_to_the_shared_folder(apps):
    own = apps.store.agent_directory(apps.bot) / "hermes" / "mcp-tokens"; own.mkdir(parents=True)
    (own / "notion.json").write_text("old")
    apps.computers.prepare(apps.bot)
    assert (apps.store.directory / "mcp-tokens" / "notion.json").read_text() == "old" and not (own / "notion.json").exists()


async def test_tools_of_a_connected_app_can_be_switched_off(apps):
    view = await apps.connectors.connect("context7", apps.bot)
    assert view["state"] == "connected" and view["disabled_tools"] == []
    view = await apps.connectors.set_tools("context7", ["search"])
    assert view["disabled_tools"] == ["search"]
    entry = apps.store.get("mcp", "shared")["context7"]
    assert "search" in entry["tools"]["exclude"]
    config = yaml.safe_load((apps.store.agent_directory(apps.bot) / "hermes" / "config.yaml").read_text())
    assert "search" in config["mcp_servers"]["context7"]["tools"]["exclude"] and "disabled_tools" not in config["mcp_servers"]["context7"]
    # Reconnecting keeps the choice; switching back on clears it.
    await apps.connectors.connect("context7", apps.bot)
    assert apps.store.get("mcp", "shared")["context7"]["disabled_tools"] == ["search"]
    view = await apps.connectors.set_tools("context7", [])
    assert view["disabled_tools"] == [] and "tools" not in apps.store.get("mcp", "shared")["context7"]
    with pytest.raises(ValueError): await apps.connectors.set_tools("notion", ["x"])


async def test_disconnecting_an_app_keeps_a_custom_connectors_sign_in(apps):
    await apps.connectors.connect("context7", apps.bot)
    apps.store.put("mcp", apps.bot, {"context7": {"url": "https://example.test/mcp", "auth": "oauth", "enabled": True}})
    token = apps.store.directory / "mcp-tokens" / "context7.json"
    token.parent.mkdir(parents=True, exist_ok=True); token.write_text("{}")
    await apps.connectors.disconnect("context7")
    assert token.exists()
    apps.store.put("mcp", apps.bot, {})
    await apps.connectors.connect("context7", apps.bot); await apps.connectors.disconnect("context7")
    assert not token.exists()
