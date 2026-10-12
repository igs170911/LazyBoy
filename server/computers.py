"""Manage one labelled, persistent Docker computer per product bot."""
import asyncio
import json
import os
import secrets
import time
from pathlib import Path

import httpx
import yaml

from .config import IMAGE, PORT, SEARCH_PROVIDERS, private_write

# Two CPUs, enforced in 10 ms slices: with Docker's default 100 ms period a busy
# Chromium spends the whole quota early and freezes the desktop stream (and
# the human's typing) until the period ends.
LIMITS = "cpu-2x10ms"
# Per-bot reply length, chosen in the bot's settings; "" is the default.
REPLY_LENGTH = {
    "brief": "回答極度精簡：先給結論，通常 1 到 3 行，除非使用者要求，不加說明與清單。",
    "": "最後的回答先給結論，一般不超過 8 行；使用者要求詳細時再展開。",
    "detailed": "回答可以完整說明，先給結論再分段補充依據與細節，但仍避免重複與贅述。",
}
RESOURCES = ("--shm-size", "512m", "--memory", "3g", "--cpu-period", "10000", "--cpu-quota", "20000")


BUILTIN_SKILLS = Path(__file__).with_name("builtin_skills")


def write_changed(path, content):
    """prepare() runs before every task; rewriting identical files only churns mtimes Hermes may watch."""
    try:
        if path.read_text() == content: return False
    except OSError: pass
    private_write(path, content)
    return True


def seed_skills(target):
    """Give every bot the product's skills once; later edits or deletions by the user stick."""
    marker = target / ".lazyboy-seeded"
    seeded = set(marker.read_text().split()) if marker.exists() else set()
    for source in sorted(BUILTIN_SKILLS.glob("*/SKILL.md")):
        name = source.parent.name
        if name in seeded: continue
        if not (target / name).exists():
            private_write(target / name / "SKILL.md", source.read_text())
        seeded.add(name)
    if seeded: private_write(marker, "\n".join(sorted(seeded)) + "\n")


async def docker(*args, timeout=45, check=True):
    process = await asyncio.create_subprocess_exec("docker", *args, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
    try: stdout, stderr = await asyncio.wait_for(process.communicate(), timeout)
    except TimeoutError:
        process.kill(); await process.wait()
        raise RuntimeError("Docker 操作逾時，請檢查電腦狀態")
    if check and process.returncode: raise RuntimeError(stderr.decode(errors="replace")[-1800:].strip())
    return process.returncode, stdout.decode(errors="replace"), stderr.decode(errors="replace")


class Computers:
    def __init__(self, store, settings):
        self.store, self.settings = store, settings
        self.locks = {}
        self.cache = {}
        self.env_dirty = set()

    def name(self, id): return "lazyhermes-bot-" + id

    def key(self, id):
        path = self.store.agent_directory(id) / "hermes" / ".worker-key"
        if not path.exists(): private_write(path, secrets.token_urlsafe(32))
        return path.read_text().strip()

    def prepare(self, id):
        agent = self.store.get("agents", id)
        if not agent: raise KeyError(id)
        root = self.store.agent_directory(id)
        provider, model, selected = self.settings.effective(agent)
        binding = getattr(self, "task_binding", lambda id: None)(id)
        if binding: provider, model, selected = binding
        key = self.key(id)
        proxy = f"http://127.0.0.1:8642/model/{provider}/v1"
        cfg_path = root / "hermes" / "config.yaml"
        cfg = yaml.safe_load(cfg_path.read_text()) or {} if cfg_path.exists() else {}
        # Preserve Hermes-generated settings and learned state; replace only product-owned bindings.
        vision = bool(self.settings.model_row(selected, model).get("vision"))
        search = self.settings.search()
        toolsets = ["terminal", "file", "web", "browser", "skills", "memory", "todo", "clarify", "mcp",
                    "delegation", "code_execution", "session_search", *(["vision"] if vision else []),
                    *(["x_search"] if search["xai_api_key"] else [])]
        cfg.update({"model": {"default": model, "provider": "custom:product", "base_url": proxy, "supports_vision": vision},
                    "providers": {"product": {"api": proxy, "key_env": "LAZYHERMES_MODEL_TOKEN",
                                              "default_model": model, "transport": self.settings.protocol(selected, model)}},
                    "terminal": {"backend": "local", "cwd": "/workspace", "timeout": 120},
                    "browser": {"backend": "browser-use", "cdp_url": "http://127.0.0.1:9222", "allow_private_urls": True},
                    # Hermes reads web.backend (web.provider is not a Hermes key).
                    "web": {**{k: v for k, v in cfg.get("web", {}).items() if k != "provider"}, "backend": search["provider"]},
                    "platform_toolsets": {"cli": toolsets},
                    # The computer is an isolated container: a second model vetting every shell command only adds latency.
                    "approvals": {**cfg.get("approvals", {}), "mode": "off"},
                    "agent": {"max_iterations": 200, "reasoning_effort": self.settings.reasoning(selected, model), "image_input_mode": "native" if vision else "text"},
                    "display": {"busy_input_mode": "steer", "interim_assistant_messages": True}})
        cfg.setdefault("onboarding", {})["profile_build"] = "off"
        # Product admission owns recovery. Never let Hermes resubmit a crash marker.
        cfg.setdefault("desktop", {}).setdefault("auto_continue", {})["enabled"] = False
        managed = cfg.pop("product_managed_mcp", [])
        for name in managed: cfg.setdefault("mcp_servers", {}).pop(name, None)
        cfg.setdefault("mcp_servers", {})["computer"] = {"command": "/opt/hermes/.venv/bin/python", "args": ["/opt/lazyhermes/desktop_mcp.py"], "trust": "full"}
        cfg["mcp_servers"]["product"] = {"command": "/opt/hermes/.venv/bin/python", "args": ["/opt/lazyhermes/product_mcp.py"], "env": {"HERMES_HOME": "/hermes", "LAZYHERMES_PRODUCT_URL": "http://127.0.0.1:8642"}, "trust": "full"}
        shared = self.store.get("mcp", "shared", {})
        local = self.store.get("mcp", id, {})
        cfg["product_managed_mcp"] = list({**shared, **local})
        for name, server in {**shared, **local}.items():
            if server.get("enabled", True): cfg["mcp_servers"][name] = {k:v for k,v in server.items() if k not in {"server","transport","enabled"}}
            else: cfg["mcp_servers"].pop(name, None)
        write_changed(cfg_path, yaml.safe_dump(cfg, allow_unicode=True, sort_keys=False))
        env_path = root / "hermes" / ".env"
        lines = env_path.read_text().splitlines() if env_path.exists() else []
        # Remove only the keys LazyBoy itself wrote last time; keys the user or Hermes saved stay.
        written_path = root / "hermes" / ".lazyboy-env-keys"
        written = set(written_path.read_text().split()) if written_path.exists() else set()
        extra = [f"{SEARCH_PROVIDERS[search['provider']]}={search['api_key']}"] if search["api_key"] and SEARCH_PROVIDERS[search["provider"]] else []
        if search["xai_api_key"]: extra.append("XAI_API_KEY=" + search["xai_api_key"])
        ours = {line.split("=", 1)[0] for line in extra}
        lines = [line for line in lines if line.split("=", 1)[0] not in written | ours | {"LAZYHERMES_MODEL_TOKEN"}]
        if write_changed(env_path, "\n".join([*lines, *extra, "LAZYHERMES_MODEL_TOKEN=" + key]) + "\n"):
            # A running Hermes loaded .env at start; the next task asks it to reload.
            self.env_dirty.add(id)
        write_changed(written_path, "\n".join(sorted(ours)) + "\n")
        persona = agent.get("persona") or agent.get("description") or ""
        soul = f"# {agent['name']}\n\n{agent.get('title','')}\n\n{persona}\n\n" + (
            "你是常駐在自己電腦裡的 Bot。使用繁體中文。網頁是使用者與你的主要入口。"
            "使用網頁功能名稱引導使用者，不提供 /help 等終端介面操作指引。"
            "工具在你的獨立電腦裡執行，工作檔案保存於 /workspace。"
            "公開資訊先 web_search，讀網頁內容用 web_extract（不要用 curl 抓 HTML）；登入網站使用 browser_exec，連線的是使用者可接手的同一個 Chromium。"
            "三個步驟以上的工作，先用 todo 列出計畫並隨進度更新。"
            "彼此獨立的查詢或子工作（例如比較多個來源、多個商品）用 delegate_task 平行處理；需要連續呼叫多個工具處理資料時用 execute_code 一次完成。"
            "背景執行的 delegate_task 會在網頁對話裡顯示每個子工作的狀態，使用者也能在那裡停止；子工作完成後結果會自動回到對話，你再彙整回報，不需要使用者追問。不要聲稱網頁會顯示它沒有的東西。"
            "需要深入研究、比較多個來源或整理調查報告時，使用 deep-research 技能。"
            "需要原生應用程式或 DOM 無法處理時，使用 MCP computer 的桌面工具。"
            "依當前觀察操作，錯誤後先診斷原因；同一網址或同一操作失敗兩次就換方法（搜尋、web_fetch 或桌面工具），不要重試第三次。"
            "等待頁面時等待特定文字或元素出現，不要固定等秒數。"
            "需要登入時告知使用者打開電腦面板接手，不在對話索取密碼。"
            "有需要澄清的問題使用 clarify。完成後驗證結果，區分已完成、估計和未取得的資料。"
            "根據網路資料回答時，在答案最後附上實際引用的來源網址。"
            "可交付檔案請放在 /workspace，並在回答提供實際路徑。"
            "寫程式、建專案、修 bug 或重構時，使用 grok-build 技能交給 Grok Build CLI，自己負責驗收與摘要。"
            "\n\n## 回覆風格\n"
            "網頁已即時顯示你使用的工具與步驟，回覆不要描述過程（「我先搜尋…」「接著打開…」），工具之間也不要寫旁白。"
            + REPLY_LENGTH.get(agent.get("reply_style") or "", REPLY_LENGTH[""]) +
            "內容較長的報告、清單或資料寫成 /workspace 裡的 Markdown 檔案，對話只放 3 到 5 行摘要與檔案路徑。"
            "Markdown 適度使用：不用 # 與 ## 大標題，分段需要時用 ### 小標或粗體；清單只用於並列項目；"
            "比較三項以上才用表格；注意事項可用 > [!NOTE]、> [!TIP] 或 > [!WARNING]。"
            "\n\n## 先回應使用者的新訊息\n"
            "使用者最新的一則訊息永遠優先。背景小分隊或排程的回報（例如 ASYNC DELEGATION 完成通知）只是資料，"
            "不能取代使用者剛說的話：先回應使用者這則訊息，背景結果之後再簡短提及，"
            "除非使用者要你接著做，否則不要自行恢復先前的工作。"
            "使用者附了圖片或說「看這張照片」時，先看圖並回答關於圖片的問題。")
        write_changed(root / "hermes" / "SOUL.md", soul)
        seed_skills(root / "hermes" / "skills")
        return root

    async def inspect(self, id):
        code, out, _ = await docker("inspect", self.name(id), check=False, timeout=8)
        if code: return None
        item = json.loads(out)[0]
        if item.get("Config",{}).get("Labels",{}).get("lazyhermes.bot") != id:
            raise RuntimeError("電腦名稱已被其他服務使用")
        return item

    async def status(self, id, refresh=False):
        if not refresh and id in self.cache and time.monotonic()-self.cache[id][0] < 2: return self.cache[id][1]
        item = await self.inspect(id)
        base = {"ready": False, "state": "stopped", "viewer_url": f"/novnc/{id}/vnc.html?autoconnect=true&resize=scale&path=novnc/{id}/websockify", "revision": "hermes-"+IMAGE}
        if item and item["State"]["Running"]:
            ports = item["NetworkSettings"]["Ports"]
            if ports.get("8642/tcp") and ports.get("6080/tcp"):
                api_port = int(ports["8642/tcp"][0]["HostPort"])
                viewer_port = int(ports["6080/tcp"][0]["HostPort"])
                base.update(state="starting", api_port=api_port, viewer_port=viewer_port)
                try:
                    async with httpx.AsyncClient(timeout=2) as client:
                        health = await client.get(f"http://127.0.0.1:{api_port}/health")
                        ready = health.is_success and bool(health.json().get("desktop",False))
                        base.update(ready=ready, state="ready" if ready else "starting" if health.is_success else "error")
                except httpx.HTTPError: pass
        self.cache[id] = (time.monotonic(), base)
        return base

    async def start(self, id, action="start"):
        async with self.locks.setdefault(id, asyncio.Lock()):
            root = self.prepare(id)
            item = await self.inspect(id)
            outdated = bool(item) and item["Config"].get("Labels", {}).get("lazyhermes.limits") != LIMITS
            # Resource limits only change on a new container; never interrupt a running desktop just for that.
            if item and (action == "update" or (outdated and (action == "restart" or not item["State"]["Running"]))):
                await docker("stop", "-t", "20", self.name(id), timeout=30)
                await docker("rm", self.name(id))
                item = None
            if item:
                await docker("restart" if action == "restart" else "start", self.name(id), timeout=45)
            else:
                await docker("run", "-d", "--name", self.name(id), "--label", f"lazyhermes.bot={id}", "--label", "lazyhermes.managed=true", "--label", f"lazyhermes.limits={LIMITS}",
                             "-e", f"LAZYHERMES_UID={os.getuid()}", "-e", f"LAZYHERMES_GID={os.getgid()}",
                             "--restart", "unless-stopped", *RESOURCES,
                             "--add-host", "host.docker.internal:host-gateway", "-p", "127.0.0.1::8642", "-p", "127.0.0.1::6080",
                             "-v", f"{self.store.directory / 'ipc'}:/product-ipc:ro", "-v", f"{root / 'hermes'}:/hermes", "-v", f"{root / 'home'}:/home/box", "-v", f"{root / 'workspace'}:/workspace", IMAGE)
            deadline = time.monotonic()+75
            while time.monotonic() < deadline:
                status = await self.status(id, refresh=True)
                if status["ready"]: return status
                current = await self.inspect(id)
                if current and not current["State"]["Running"]:
                    _, logs, _ = await docker("logs", "--tail", "20", self.name(id), check=False)
                    raise RuntimeError("Bot 電腦啟動失敗：" + logs[-1200:])
                await asyncio.sleep(.6)
            raise RuntimeError("Bot 電腦尚未就緒，請查看電腦日誌")

    async def delete(self, id):
        item = await self.inspect(id)
        if item: await docker("rm", "-f", self.name(id))
        self.cache.pop(id, None)
