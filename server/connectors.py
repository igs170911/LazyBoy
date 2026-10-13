"""Connect apps: the curated MCP catalog Hermes ships (vendor/hermes-agent/optional-mcps), connected once for all bots.

Every entry is a vendor-hosted MCP server. Connecting one adds it to the shared connectors. Apps that sign in with
OAuth do it on a bot's own computer: Hermes binds a one-shot loopback receiver inside that container and the
authorization page opens in the bot's browser next to it, so the redirect lands without publishing a port. When
the person signs in on their own device instead, the page they land on fails to load; pasting its address here
relays it to the same receiver. Tokens are written to /hermes/mcp-tokens, which every bot mounts from one shared
folder, so one sign-in serves all bots and a token one bot refreshes is the one the others read next.
"""
import asyncio
import contextlib
import functools
import re
import time
from pathlib import Path
from urllib.parse import urlparse

import httpx
import yaml

from . import computers as computers_module
from .config import ROOT
from .store import now

CATALOG_DIR = ROOT / "vendor" / "hermes-agent" / "optional-mcps"
SHARED = "shared"
CATEGORIES = ["工作", "設計", "開發", "商務", "生活"]
FEATURED = ["notion", "atlassian", "linear", "github", "figma", "canva", "dropbox", "todoist", "context7"]
# Entries that need the person's own developer app (asana), their own server address (n8n) or a local editor.
SKIP = {"asana", "n8n-official", "unreal-engine"}
# id: (name shown, what it lets the bot do, category, site for the icon)
APPS = {
    "airtable": ("Airtable", "讀寫 Airtable 的資料表與紀錄", "工作", "airtable.com"),
    "algolia": ("Algolia", "查看搜尋索引、分析與設定（唯讀）", "開發", "algolia.com"),
    "alltrails": ("AllTrails", "找登山步道、評價與評分", "生活", "alltrails.com"),
    "amplitude": ("Amplitude", "產品分析圖表、儀表板與實驗", "商務", "amplitude.com"),
    "atlassian": ("Jira 與 Confluence", "Atlassian 的 Jira 議題與 Confluence 頁面", "工作", "atlassian.com"),
    "attio": ("Attio", "CRM 客戶紀錄、清單與筆記", "商務", "attio.com"),
    "aws-knowledge": ("AWS 文件", "查詢 AWS 官方文件與最佳做法", "開發", "aws.amazon.com"),
    "betterstack": ("Better Stack", "日誌、監控、事件與狀態頁", "開發", "betterstack.com"),
    "buildkite": ("Buildkite", "CI/CD 管線、建置與測試結果", "開發", "buildkite.com"),
    "calendly": ("Calendly", "預約連結、活動與受邀者", "工作", "calendly.com"),
    "canva": ("Canva", "建立、搜尋與管理 Canva 設計", "設計", "canva.com"),
    "circleci": ("CircleCI", "診斷建置失敗、讀取日誌、重跑流程", "開發", "circleci.com"),
    "clickup": ("ClickUp", "ClickUp 的任務、文件與工作區", "工作", "clickup.com"),
    "close": ("Close", "銷售 CRM：潛在客戶、商機、通話與郵件", "商務", "close.com"),
    "cloudflare": ("Cloudflare", "管理網域、Workers 與 Cloudflare 設定", "開發", "cloudflare.com"),
    "cloudinary": ("Cloudinary", "上傳、搜尋與轉換圖片影片素材", "設計", "cloudinary.com"),
    "comfy-cloud": ("Comfy Cloud", "生成圖片、影片、音訊與 3D", "設計", "comfy.org"),
    "context7": ("Context7", "查詢套件最新、對應版本的技術文件", "開發", "context7.com"),
    "craft": ("Craft", "Craft 文件、任務與個人知識庫", "工作", "craft.do"),
    "datadog": ("Datadog", "日誌、監控、儀表板與事件", "開發", "datadoghq.com"),
    "deepwiki": ("DeepWiki", "詢問任何公開 GitHub 專案的問題", "開發", "deepwiki.com"),
    "dropbox": ("Dropbox", "搜尋、讀取與管理 Dropbox 檔案", "工作", "dropbox.com"),
    "figma": ("Figma", "讀取設計稿、元件與 Code Connect", "設計", "figma.com"),
    "fireflies": ("Fireflies", "會議逐字稿、摘要與待辦事項", "工作", "fireflies.ai"),
    "gamma": ("Gamma", "用 AI 生成與編輯簡報、文件和網站", "設計", "gamma.app"),
    "gitlab": ("GitLab", "議題、Merge Request、管線與程式碼", "開發", "gitlab.com"),
    "globalping": ("Globalping", "從全球節點測 ping、traceroute 與 DNS", "開發", "globalping.io"),
    "grafana": ("Grafana", "查詢指標、日誌、儀表板與警示", "開發", "grafana.com"),
    "hugging_face": ("Hugging Face", "模型、資料集、Spaces 與論文", "開發", "huggingface.co"),
    "indeed": ("Indeed", "搜尋職缺", "生活", "indeed.com"),
    "intercom": ("Intercom", "客服對話、工單與客戶資料", "商務", "intercom.com"),
    "kiwi": ("Kiwi.com", "搜尋機票行程與訂票連結", "生活", "kiwi.com"),
    "klaviyo": ("Klaviyo", "行銷活動、流程、分眾與報表", "商務", "klaviyo.com"),
    "linear": ("Linear", "查詢、建立與更新議題和專案", "工作", "linear.app"),
    "microsoft-learn": ("Microsoft Learn", "微軟、Azure、.NET 官方文件與範例", "開發", "learn.microsoft.com"),
    "miro": ("Miro", "讀寫 Miro 白板、圖表與框架", "設計", "miro.com"),
    "mixpanel": ("Mixpanel", "事件、漏斗、留存與儀表板", "商務", "mixpanel.com"),
    "monday": ("monday.com", "看板、項目、文件與工作流程", "工作", "monday.com"),
    "motherduck": ("MotherDuck", "用 SQL 查詢 DuckDB 雲端資料倉儲", "開發", "motherduck.com"),
    "neon": ("Neon", "Serverless Postgres 專案、分支與 SQL", "開發", "neon.tech"),
    "netlify": ("Netlify", "網站、部署與環境變數", "開發", "netlify.com"),
    "notion": ("Notion", "Notion 工作區的頁面與資料庫", "工作", "notion.so"),
    "paypal": ("PayPal", "付款、發票與訂閱", "商務", "paypal.com"),
    "plaid": ("Plaid", "Plaid 後台：整合、Items 與用量", "商務", "plaid.com"),
    "postman": ("Postman", "工作區、API 集合與環境", "開發", "postman.com"),
    "prisma-postgres": ("Prisma Postgres", "建立與管理 Prisma Postgres 資料庫", "開發", "prisma.io"),
    "railway": ("Railway", "專案、服務、部署與環境", "開發", "railway.com"),
    "robinhood": ("Robinhood", "投資組合、餘額與下單", "商務", "robinhood.com"),
    "semgrep": ("Semgrep", "掃描程式碼的資安漏洞", "開發", "semgrep.dev"),
    "sentry": ("Sentry", "錯誤議題、堆疊追蹤與上下文", "開發", "sentry.io"),
    "square": ("Square", "商品目錄、訂單與付款", "商務", "squareup.com"),
    "strava": ("Strava", "運動紀錄、體能趨勢與訓練負荷（唯讀）", "生活", "strava.com"),
    "stripe": ("Stripe", "付款、客戶與發票", "商務", "stripe.com"),
    "supabase": ("Supabase", "資料庫、登入驗證與儲存空間", "開發", "supabase.com"),
    "todoist": ("Todoist", "管理 Todoist 任務與專案", "工作", "todoist.com"),
    "trivago": ("trivago", "依城市與日期比較飯店價格", "生活", "trivago.com"),
    "twelve-data": ("Twelve Data", "股票、外匯與加密貨幣行情", "生活", "twelvedata.com"),
    "twilio-docs": ("Twilio 文件", "搜尋 Twilio 開發文件（唯讀）", "開發", "twilio.com"),
    "vercel": ("Vercel", "部署、日誌與專案", "開發", "vercel.com"),
    "webflow": ("Webflow", "網站、CMS 集合與頁面", "設計", "webflow.com"),
    "wolfram": ("Wolfram|Alpha", "計算、數學與知識查詢", "生活", "wolframalpha.com"),
    "wordpress-com": ("WordPress.com", "文章、頁面、草稿、統計與留言", "設計", "wordpress.com"),
}
# Not in Hermes' catalog: GitHub's hosted server has no self-service OAuth, so it takes a personal access token.
EXTRA = {
    "github": {"id": "github", "title": "GitHub", "description": "程式碼、議題與 Pull Request", "category": "開發", "site": "github.com",
               "url": "https://api.githubcopilot.com/mcp/", "auth": "token", "token_header": "Authorization",
               "token_hint": "GitHub Personal Access Token", "token_url": "https://github.com/settings/personal-access-tokens/new",
               "keywords": ["github", "pull request"], "hosts": ["github.com"], "exclude": []},
}
ICON_BYTES = 256 * 1024
OAUTH_TIMEOUT = 15 * 60


def safe_filename(name):
    """Hermes' token file stem for a server name (tools/mcp_oauth.py)."""
    return re.sub(r"[^\w\-]", "_", name).strip("_")[:128] or "default"


@functools.lru_cache(maxsize=1)
def catalog():
    """Catalog entries in display order: Hermes' manifests merged with the product's names and categories."""
    items = dict(EXTRA)
    for path in sorted(CATALOG_DIR.glob("*/manifest.yaml")):
        try: manifest = yaml.safe_load(path.read_text()) or {}
        except (OSError, yaml.YAMLError): continue
        name = manifest.get("name") or path.parent.name
        transport, auth = manifest.get("transport") or {}, manifest.get("auth") or {}
        url = str(transport.get("url") or "")
        if name in SKIP or name not in APPS or transport.get("type") != "http" or "${" in url or auth.get("env"): continue
        title, description, category, site = APPS[name]
        suggest = manifest.get("suggest") or {}
        items[name] = {"id": name, "title": title, "description": description, "category": category, "site": site, "url": url,
                       "auth": "oauth" if auth.get("type") == "oauth" else "none",
                       "keywords": [str(k) for k in suggest.get("keywords") or []], "hosts": [str(h) for h in suggest.get("hosts") or []],
                       "exclude": [str(t) for t in (manifest.get("tools") or {}).get("default_excluded") or []]}
    rank = {id: i for i, id in enumerate(FEATURED)}
    return dict(sorted(items.items(), key=lambda kv: (rank.get(kv[0], len(rank)), kv[1]["title"].lower())))


def server_config(item, disabled=()):
    """The shared connector entry for a catalog app; `catalog` marks it so the settings show it as an app.
    `disabled` are tools the person switched off (Grok: individual plugin tools can be turned off)."""
    entry = {"url": item["url"], "catalog": item["id"], "enabled": True}
    if item["auth"] == "oauth": entry["auth"] = "oauth"
    exclude = [*item["exclude"], *[t for t in disabled if t not in item["exclude"]]]
    if exclude: entry["tools"] = {"exclude": exclude}
    if disabled: entry["disabled_tools"] = sorted(set(disabled))
    return entry


class Connectors:
    def __init__(self, store, runtime, computers):
        self.store, self.runtime, self.computers = store, runtime, computers
        self.tokens = store.directory / "mcp-tokens"
        self.icons = store.directory / "connector-icons"
        # name → {"agent", "session", "auth_url", "started"}: sign-ins waiting for the person.
        self.flows = {}

    # ── shared helpers (also used by the custom-MCP settings) ─────────────────────────────────────────────

    def has_token(self, name):
        return (self.tokens / f"{safe_filename(name)}.json").exists()

    def bots_for(self, scope):
        return [a["id"] for a in self.store.all("agents")] if scope == SHARED else [scope]

    async def apply(self, scope):
        """Write the changed connectors into each affected bot's config and reload MCP where nothing is running."""
        for id in self.bots_for(scope):
            with contextlib.suppress(KeyError): self.computers.prepare(id)
            client = self.runtime.clients.get(id)
            if client and client.alive and not self.runtime.busy(id):
                with contextlib.suppress(Exception):
                    await client.call("reload.mcp", {"confirm": True, "session_id": getattr(client, "product_session", None)})
                    self.computers.mcp_dirty.discard(id)

    def pick_bot(self, scope, prefer=None, idle=False):
        """A bot to run a check or a sign-in on: the one asked for, else one whose computer is already up and idle."""
        candidates = self.bots_for(scope)
        if not candidates: raise ValueError("請先建立 Bot")
        live = [id for id in candidates if (c := self.runtime.clients.get(id)) and c.alive]
        free = [id for id in live if not self.runtime.busy(id)]
        if prefer in candidates and (not idle or not self.runtime.busy(prefer)): return prefer
        return (free or ([] if idle else live) or candidates)[0]

    def remember(self, scope, name, ok, tools=(), error=None):
        rows = [{"name": t.get("name"), "description": t.get("description")} if isinstance(t, dict) else {"name": str(t), "description": ""} for t in tools or []]
        self.store.put("mcp_status", f"{scope}:{name}", {"ok": bool(ok), "error": error, "tools": rows, "at": now()})

    async def test(self, scope, name, prefer=None, auto=False):
        """Connect once and list the tools on a bot that has this connector; the result is remembered for the settings."""
        if name not in self.store.get("mcp", scope, {}): raise ValueError("找不到這個串接")
        candidates = self.bots_for(scope)
        live = [id for id in candidates if (c := self.runtime.clients.get(id)) and c.alive]
        if auto and not live: return False
        id = self.pick_bot(scope, prefer)
        try:
            self.computers.prepare(id); client = await self.runtime.client(id)
            result = await client.call("mcp.servers.test", {"name": name}, timeout=90)
        except Exception as exc:  # remembered like any failed test, so the card says why
            result = {"ok": False, "error": str(exc)[:500] or "連線測試沒有回應"}
        error = result.get("error")
        if result.get("oauth_needed") and not result.get("ok"): error = "需要登入"
        self.remember(scope, name, result.get("ok"), result.get("tools"), error)
        return True

    # ── the app gallery ───────────────────────────────────────────────────────────────────────────────────

    def view(self, item):
        entry = self.store.get("mcp", SHARED, {}).get(item["id"])
        mine = bool(entry and entry.get("catalog") == item["id"])
        status = self.store.get("mcp_status", f"{SHARED}:{item['id']}") if mine else None
        flow = self.flows.get(item["id"])
        if not mine: state = "available"
        elif flow: state = "signing_in"
        elif entry.get("enabled") is False: state = "off"
        elif item["auth"] == "oauth" and not self.has_token(item["id"]): state = "needs_signin"
        elif status and not status.get("ok"): state = "needs_signin" if status.get("error") == "需要登入" else "error"
        elif status: state = "connected"
        else: state = "checking"
        public = {k: item[k] for k in ("id", "title", "description", "category", "auth", "keywords", "hosts", "token_hint", "token_url") if k in item}
        return {**public, "featured": item["id"] in FEATURED, "state": state, "tools": (status or {}).get("tools") or [],
                "disabled_tools": (entry or {}).get("disabled_tools", []) if mine else [],
                "error": (status or {}).get("error") if state == "error" else None, "checked_at": (status or {}).get("at"),
                "signin": {"agent": flow["agent"], "auth_url": flow["auth_url"]} if flow else None}

    def listing(self):
        return {"apps": [self.view(item) for item in catalog().values()], "categories": CATEGORIES}

    def item(self, id):
        item = catalog().get(id)
        if not item: raise KeyError(id)
        return item

    async def connect(self, id, prefer=None, token=""):
        item = self.item(id)
        entries = self.store.get("mcp", SHARED, {})
        current = entries.get(id)
        if current and current.get("catalog") != id: raise ValueError(f"已有名為「{id}」的自訂串接，請先在下方移除它")
        entry = {**server_config(item, (current or {}).get("disabled_tools", ())), **({k: current[k] for k in ("headers",) if current and k in current})}
        if item["auth"] == "token":
            token = str(token or "").strip()
            if token: entry["headers"] = {item["token_header"]: token if token.lower().startswith("bearer ") else f"Bearer {token}"}
            if not entry.get("headers"): raise ValueError(f"請貼上 {item['token_hint']}")
        entries[id] = entry
        self.store.put("mcp", SHARED, entries)
        self.store.remove("mcp_status", f"{SHARED}:{id}")
        await self.apply(SHARED)
        if item["auth"] != "oauth" or self.has_token(id):
            await self.test(SHARED, id, prefer)
            if item["auth"] != "oauth" or self.store.get("mcp_status", f"{SHARED}:{id}", {}).get("ok"): return self.view(item)
        return await self.sign_in_app(id, prefer)

    async def sign_in_app(self, id, prefer=None):
        item = self.item(id)
        if item["auth"] != "oauth": raise ValueError("這個應用程式不需要登入")
        await self.sign_in(SHARED, id, prefer, item["title"])
        return self.view(item)

    async def poll_app(self, id):
        await self.poll(id, self.item(id)["title"])
        return self.view(self.item(id))

    async def relay_app(self, id, address):
        await self.relay(id, address, self.item(id)["title"])
        return self.view(self.item(id))

    async def set_tools(self, id, disabled):
        """Switch individual tools of a connected app off (or back on) for every bot."""
        item = self.item(id)
        entries = self.store.get("mcp", SHARED, {})
        entry = entries.get(id)
        if not entry or entry.get("catalog") != id: raise ValueError("請先連接這個應用程式")
        names = sorted({str(name).strip() for name in disabled or [] if str(name).strip()})[:500]
        entries[id] = {**server_config(item, names), **{k: entry[k] for k in ("headers",) if k in entry}, "enabled": entry.get("enabled", True)}
        self.store.put("mcp", SHARED, entries)
        await self.apply(SHARED)
        return self.view(item)

    async def disconnect(self, id):
        item = self.item(id)
        await self.cancel(id)
        entries = self.store.get("mcp", SHARED, {})
        if entries.get(id, {}).get("catalog") == id:
            entries.pop(id); self.store.put("mcp", SHARED, entries)
        self.store.remove("mcp_status", f"{SHARED}:{id}")
        # Tokens are kept per connector name: another bot's own connector with this name still signs in with them.
        if not any(id in self.store.get("mcp", owner, {}) for owner in [SHARED, *[a["id"] for a in self.store.all("agents")]]):
            self.forget_tokens(id)
        await self.apply(SHARED)
        return self.view(item)

    # ── OAuth sign-in for any remote connector (catalog apps and custom ones) ──────────────────────────────

    def signin_state(self, name):
        flow = self.flows.get(name)
        return {"agent": flow["agent"], "auth_url": flow["auth_url"]} if flow else None

    async def sign_in(self, scope, name, prefer=None, title=None):
        """Start the connector's OAuth sign-in on a bot's computer and open the page in that bot's browser."""
        entry = self.store.get("mcp", scope, {}).get(name)
        if not entry or not entry.get("url"): raise ValueError("只有遠端網址的串接可以登入")
        title = title or name
        await self.cancel(name)
        bot = self.pick_bot(scope, prefer, idle=True)
        self.computers.prepare(bot)
        client = await self.runtime.client(bot)
        try: result = await client.call("mcp.servers.oauth.start", {"name": name}, timeout=60)
        except Exception as exc: raise ValueError(f"無法開始登入 {title}：{str(exc)[:300]}") from exc
        if not result.get("auth_url"): raise ValueError(f"{title} 沒有回傳登入網址")
        self.flows[name] = {"scope": scope, "agent": bot, "session": result["session_id"], "auth_url": result["auth_url"], "started": time.monotonic()}
        await self.open_page(bot, result["auth_url"])
        return self.signin_state(name)

    async def open_page(self, bot, url):
        with contextlib.suppress(Exception):
            await computers_module.docker("exec", "-d", "-u", computers_module.user_flag(), "-e", "DISPLAY=:1",
                                          self.computers.name(bot), "/usr/local/bin/box-chrome", url, timeout=20)

    async def poll(self, name, title=None):
        """"pending" while the person signs in, "approved" once tokens are saved, "idle" with nothing in progress."""
        flow = self.flows.get(name)
        if not flow: return "idle"
        title = title or name
        if time.monotonic() - flow["started"] > OAUTH_TIMEOUT:
            await self.cancel(name)
            raise ValueError("登入等太久已取消，請重新連接")
        try:
            client = await self.runtime.client(flow["agent"])
            result = await client.call("mcp.servers.oauth.poll", {"name": name, "session_id": flow["session"]}, timeout=20)
        except Exception as exc:
            raise ValueError(f"查不到登入進度：{str(exc)[:200]}") from exc
        if result.get("status") == "approved":
            self.flows.pop(name, None)
            await self.apply(flow["scope"])
            self.remember(flow["scope"], name, True, result.get("tools"))
            if not self.store.get("mcp_status", f"{flow['scope']}:{name}", {}).get("tools"): await self.test(flow["scope"], name, flow["agent"])
            return "approved"
        if result.get("status") == "error":
            self.flows.pop(name, None)
            message = result.get("error_message") or "登入沒有完成"
            if "cancel" in message.lower(): return "idle"
            raise ValueError(f"{title} 登入失敗：{message[:300]}")
        return "pending"

    async def relay(self, name, address, title=None):
        """The person signed in on their own device and pasted the address the provider sent them back to."""
        flow = self.flows.get(name)
        if not flow: raise ValueError("目前沒有進行中的登入，請重新連接")
        parsed = urlparse(str(address or "").strip())
        if (parsed.scheme != "http" or parsed.hostname not in ("127.0.0.1", "localhost") or not parsed.port
                or parsed.path.rstrip("/") not in ("/callback", "") or ("code=" not in parsed.query and "error=" not in parsed.query)):
            raise ValueError("請貼上登入後網址列的完整網址（以 http://127.0.0.1 或 http://localhost 開頭）")
        target = f"http://127.0.0.1:{parsed.port}/callback?{parsed.query}"
        code, _, err = await computers_module.docker("exec", self.computers.name(flow["agent"]), "curl", "-sS", "--max-time", "15",
                                                     "-o", "/dev/null", target, check=False, timeout=25)
        if code: raise ValueError("這個登入已失效，請重新連接後再試" if "refused" in err.lower() else f"轉交登入結果失敗：{err[:200]}")
        state = "pending"
        for _ in range(20):
            state = await self.poll(name, title)
            if state != "pending": break
            await asyncio.sleep(.5)
        return state

    async def cancel(self, name):
        flow = self.flows.pop(name, None)
        if not flow: return
        client = self.runtime.clients.get(flow["agent"])
        if client and client.alive:
            with contextlib.suppress(Exception):
                await client.call("mcp.servers.oauth.cancel", {"name": name, "session_id": flow["session"]}, timeout=10)

    def forget_tokens(self, name):
        stem = safe_filename(name)
        for suffix in (".json", ".client.json", ".meta.json"):
            with contextlib.suppress(FileNotFoundError): (self.tokens / f"{stem}{suffix}").unlink()

    async def icon(self, id):
        """The app's icon, fetched once from its own site (or DuckDuckGo's favicon service) and kept on disk."""
        item = self.item(id)
        self.icons.mkdir(parents=True, exist_ok=True)
        # A real icon wins over the marker left by an earlier failed fetch.
        cached = min(self.icons.glob(f"{safe_filename(id)}.*"), key=lambda path: path.suffix == ".none", default=None)
        # An icon that could not be fetched is tried again the next day.
        if cached and (cached.suffix != ".none" or time.time() - cached.stat().st_mtime < 86400): return cached
        missing = self.icons / f"{safe_filename(id)}.none"
        sources = [f"https://{item['site']}/apple-touch-icon.png", f"https://icons.duckduckgo.com/ip3/{item['site']}.ico"]
        async with httpx.AsyncClient(timeout=6, follow_redirects=True, headers={"User-Agent": "Mozilla/5.0 LazyBoy"}) as client:
            for url in sources:
                with contextlib.suppress(httpx.HTTPError):
                    response = await client.get(url)
                    kind = response.headers.get("content-type", "").split(";")[0].strip()
                    if response.is_success and kind.startswith("image/") and 0 < len(response.content) <= ICON_BYTES:
                        suffix = {"image/png": ".png", "image/svg+xml": ".svg", "image/jpeg": ".jpg", "image/webp": ".webp"}.get(kind, ".ico")
                        path = self.icons / f"{safe_filename(id)}{suffix}"
                        path.write_bytes(response.content)
                        missing.unlink(missing_ok=True)
                        return path
        missing.write_bytes(b"")
        return missing
