"""Manage one labelled, persistent Docker computer per product bot."""
import asyncio
import contextlib
import json
import os
import secrets
import shutil
import subprocess
import time
from pathlib import Path

import httpx
import yaml

YAML_LOADER = getattr(yaml, "CSafeLoader", yaml.SafeLoader)

from .config import DATA, DATA_VOLUME, IMAGE, NETWORK, PORT, ROOT, SEARCH_PROVIDERS, private_write
from .skills import Skills

# Two CPUs, enforced in 10 ms slices: with Docker's default 100 ms period a busy
# Chromium spends the whole quota early and freezes the desktop stream (and
# the human's typing) until the period ends.
LIMITS = "cpu-2x10ms"
# Folders every bot shares; a container made before a change here is recreated the next time it starts or restarts.
# mcp-tokens: OAuth sign-ins for connected apps, so one sign-in serves every bot (server/connectors.py).
MOUNTS = "mcp-tokens-1"
# Where a computer's folders come from: host folders, or the product's Docker volume when LazyBoy runs in Docker.
# A computer made for the other kind cannot see this product's files, so it is replaced right away.
STORAGE = "volume" if DATA_VOLUME else "host"
# Per-bot reply length, chosen in the bot's settings; "" is the default.
# Grok's approval boundaries: actions with outside effects this bot must clear with the user first.
ASK_FIRST_ACTIONS = {"send": "寄出或傳送訊息給別人（Email、聊天、留言、私訊）", "publish": "公開發佈或修改公開內容（貼文、網站、商品頁）",
                     "pay": "付款、購買或任何會花錢的操作", "delete": "刪除或覆寫資料、檔案與雲端內容"}


def ask_first_text(chosen):
    actions = [ASK_FIRST_ACTIONS[key] for key in ("send", "publish", "pay", "delete") if key in (chosen or [])]
    if not actions: return ""
    return ("以下動作執行前，一定先取得使用者同意：" + "；".join(actions) + "。"
            "使用者正在對話時用 clarify 說明要做什麼（對象、內容、金額或範圍）並等回覆；定時任務、隊友交辦或背景工作時，"
            "改用 request_approval 送到「待你核定」（不會過期），核准結果回來後才執行；沒有同意就停在草稿或準備好的狀態。")


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


_remapped = None


def remapped_docker():
    """Rootless Docker or userns-remap: UIDs inside a container are not host UIDs, so a container chown to the
    host user's UID hands files to some other host UID. There the container's root already is the host user."""
    global _remapped
    if _remapped is None:
        try:
            out = subprocess.run(["docker", "info", "--format", "{{json .SecurityOptions}}"], capture_output=True, text=True, timeout=15).stdout
            _remapped = "rootless" in out or "userns" in out
        except (OSError, subprocess.TimeoutExpired):
            return False
    return _remapped


def container_user():
    """The UID:GID the bot's processes run as inside its computer; 0 keeps the container's root.
    In a Docker volume, the product and its computers share one user namespace: same UID, whatever the host does."""
    return (0, 0) if not DATA_VOLUME and remapped_docker() else (os.getuid(), os.getgid())


def user_flag():
    """`docker exec -u` for commands that write the bot's files: the same user its computer runs as."""
    uid, gid = container_user()
    return f"{uid}:{gid}"


def mount(source, target, readonly=False):
    """A product folder inside a computer: a host folder, or the same folder inside the product's Docker volume."""
    if DATA_VOLUME:
        subpath = Path(source).relative_to(DATA).as_posix()
        return ["--mount", f"type=volume,src={DATA_VOLUME},dst={target},volume-subpath={subpath}" + (",readonly" if readonly else "")]
    return ["-v", f"{source}:{target}" + (":ro" if readonly else "")]


def reclaim(root):
    """Give a bot's files back to the host user after a remapped container took them (its root is the host user)."""
    if DATA_VOLUME or not remapped_docker(): return False
    mounts = [arg for name in ("hermes", "home", "workspace") for arg in mount(root / name, f"/reclaim/{name}")]
    result = subprocess.run(["docker", "run", "--rm", "--user", "0:0", "--entrypoint", "chown", *mounts, IMAGE, "-R", "0:0", "/reclaim"],
                            capture_output=True, text=True, timeout=120)
    return result.returncode == 0


async def docker(*args, timeout=45, check=True, input=None):
    process = await asyncio.create_subprocess_exec("docker", *args, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
                                                   stdin=asyncio.subprocess.PIPE if input is not None else None)
    try: stdout, stderr = await asyncio.wait_for(process.communicate(input), timeout)
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
        self.keys = {}
        self.env_dirty = set()
        # Bots whose product tools changed since their Hermes loaded them; the next task reloads MCP.
        self.mcp_dirty = set()
        # Set by Collaboration: the teammate section of each bot's SOUL.
        self.roster = lambda id: ""

    def name(self, id): return "lazyhermes-bot-" + id

    def key(self, id):
        # Asked on every model request a bot makes; the file never changes once written.
        cached = self.keys.get(id)
        if cached: return cached
        path = self.store.agent_directory(id) / "hermes" / ".worker-key"
        if not path.exists(): private_write(path, secrets.token_urlsafe(32))
        value = self.keys[id] = path.read_text().strip()
        return value

    def shared_tokens(self):
        path = self.store.directory / "mcp-tokens"
        path.mkdir(mode=0o700, parents=True, exist_ok=True)
        return path

    def adopt_tokens(self, root):
        """Sign-ins a bot saved in its own home before the shared folder existed move there, unless already present."""
        own, shared = root / "hermes" / "mcp-tokens", self.shared_tokens()
        # Made here so Docker does not create the mount point as root.
        own.mkdir(parents=True, exist_ok=True)
        for path in own.iterdir():
            if path.is_file() and not (shared / path.name).exists():
                with contextlib.suppress(OSError): shutil.move(str(path), shared / path.name)

    def prepare(self, id):
        try: return self._prepare(id)
        except PermissionError:
            # Files a remapped container chowned away; take them back once and write again.
            if not reclaim(self.store.agent_directory(id)): raise
            return self._prepare(id)

    def _prepare(self, id):
        agent = self.store.get("agents", id)
        if not agent: raise KeyError(id)
        root = self.store.agent_directory(id)
        self.adopt_tokens(root)
        provider, model, selected = self.settings.effective(agent)
        binding = getattr(self, "task_binding", lambda id: None)(id)
        if binding: provider, model, selected = binding
        key = self.key(id)
        proxy = f"http://127.0.0.1:8642/model/{provider}/v1"
        cfg_path = root / "hermes" / "config.yaml"
        cfg = yaml.load(cfg_path.read_text(), Loader=YAML_LOADER) or {} if cfg_path.exists() else {}
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
        # A bot people keep talking to for days piles up old web pages and tool output, all resent on every call:
        # each step gets slow and costly long before Hermes' own trigger (75% of a 200K window). Old bulky tool
        # results are trimmed without a model call past 48K tokens, and the conversation is summarized at 100K.
        compression = cfg.setdefault("compression", {})
        compression.setdefault("proactive_prune_tokens", 48000)
        compression.setdefault("threshold_tokens", 100000)
        # Product admission owns recovery. Never let Hermes resubmit a crash marker.
        cfg.setdefault("desktop", {}).setdefault("auto_continue", {})["enabled"] = False
        managed = cfg.pop("product_managed_mcp", [])
        for name in managed: cfg.setdefault("mcp_servers", {}).pop(name, None)
        cfg.setdefault("mcp_servers", {})["computer"] = {"command": "/opt/hermes/.venv/bin/python", "args": ["/opt/lazyhermes/desktop_mcp.py"], "trust": "full"}
        # The product tools ship from this checkout into the bot's mounted home, so new tools (message_bot)
        # reach running computers without rebuilding the image.
        tools_changed = write_changed(root / "hermes" / "lazyhermes" / "product_mcp.py", (ROOT / "box" / "product_mcp.py").read_text())
        cfg["mcp_servers"]["product"] = {"command": "/opt/hermes/.venv/bin/python", "args": ["/hermes/lazyhermes/product_mcp.py"], "env": {"HERMES_HOME": "/hermes", "LAZYHERMES_PRODUCT_URL": "http://127.0.0.1:8642"}, "trust": "full"}
        shared = self.store.get("mcp", "shared", {})
        local = self.store.get("mcp", id, {})
        cfg["product_managed_mcp"] = list({**shared, **local})
        for name, server in {**shared, **local}.items():
            if server.get("enabled", True): cfg["mcp_servers"][name] = {k:v for k,v in server.items() if k not in {"server","transport","enabled","catalog","disabled_tools"}}
            else: cfg["mcp_servers"].pop(name, None)
        # Hermes sends the name and summary of every listed skill with each request. Built-in skills this bot has not
        # used lately wait in its skill library instead, found with skill_library when a task needs one.
        library = Skills(self.store).apply_library(id, cfg)
        if write_changed(cfg_path, yaml.safe_dump(cfg, allow_unicode=True, sort_keys=False)) or tools_changed:
            self.mcp_dirty.add(id)
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
            "你是常駐在自己電腦裡的 Bot，像個隨時待命的工作夥伴。一律使用繁體中文（台灣用語），回覆、檔案與替使用者寫的貼文都一樣；來源是簡體時改寫成繁體。"
            "網頁是使用者與你的主要入口。使用網頁功能名稱引導使用者，不提供 /help 等終端介面操作指引。"
            "\n\n## 即時回應與工作節奏\n"
            "使用者送出訊息後，**兩秒內必須回應**。閒聊、查詢或簡單問題直接回答；需要多步驟工作時，先用一句話確認收到並說明第一步，例如："
            "「收到，我先搜尋最新科技新聞，再整理重點給你。」「好，我現在打開 Google 首頁並截圖。」「了解，我會搜尋相關資料後整理成報告。」"
            "確認後再開始實際操作。需要工具與深入研究的工作，不要一收到就開始呼叫工具，先確認、再執行。"
            "\n\n進度更新只在**重要里程碑**回報，不要命令級的旁白："
            "- 好：「找到 5 篇相關文章，正在比對內容」「已完成前三項，最後一項資料庫查詢中」"
            "- 不好：「正在執行 web_search」「正在讀取網頁」「正在寫入檔案」"
            "\n\n工具在你的獨立電腦裡執行，工作檔案保存於 /workspace。"
            "公開資訊先 web_search，讀網頁內容用 web_extract（不要用 curl 抓 HTML）；登入網站使用 browser_exec，連線的是使用者可接手的同一個 Chromium。"
            "三個步驟以上的工作，先用 todo 列出計畫並隨進度更新。"
            "彼此獨立的查詢或子工作（例如比較多個來源、多個商品）用 delegate_task 平行處理；需要連續呼叫多個工具處理資料時用 execute_code 一次完成。"
            "背景執行的 delegate_task 會在網頁對話裡顯示每個子工作的狀態，使用者也能在那裡停止；子工作完成後結果會自動回到對話，你再彙整回報，不需要使用者追問。不要聲稱網頁會顯示它沒有的東西。"
            "需要深入研究、比較多個來源或整理調查報告時，使用 deep-research 技能。"
            "需要原生應用程式或 DOM 無法處理時，使用 MCP computer 的桌面工具。"
            "使用者連接的應用程式（Notion、Jira、Linear、GitHub、Figma、Canva、Dropbox 等）會以 MCP 工具出現，讀寫這些服務優先用它們。"
            "使用者要你處理這類服務但工具清單裡沒有時，告訴他可以到「設定 → 連接應用程式」連接，登入一次所有 Bot 都能用。"
            "使用者訊息裡的「@名字」：是隊友 Bot 就用 message_bot 請它處理或協助；是群組對話就用 post_to_group 在那個群組發言；是已連接的應用程式就用它的工具。"
            "「使用「某某」技能」表示先載入那個技能，照它的步驟做。"
            "依當前觀察操作，錯誤後先診斷原因。同一個做法失敗兩次，就換另一個做法，不要用同一個做法重試第三次："
            "例如改讀頁面 HTML 或網路請求、改點別的按鈕或入口、重新整理或開新分頁、用桌面工具截圖後操作、換關鍵字或篩選、用 web_search／web_fetch 從別的來源取得。"
            "網頁輸入框用程式填字（設定 value、textContent、insertText）後按鈕沒出現或內容沒存，是因為多數社群與表單的編輯器只認真實輸入："
            "改用真實的滑鼠點擊加逐字鍵盤輸入（browser_exec 裡用 page.mouse／page.keyboard.type，或桌面工具 desktop_act 的點擊與打字），送出前確認文字與送出按鈕都出現。"
            "送出按鈕常常只是圖示（箭頭、紙飛機、勾勾）而沒有「發佈」「送出」字樣：找不到字就看 aria-label、輸入框旁邊新出現的按鈕，或截圖辨認。"
            "換過幾種做法都不行，才回報卡在哪一步、試過什麼。"
            "等待頁面時等待特定文字或元素出現，不要固定等秒數。"
            "需要真人做一小步時（登入、驗證碼、防機器人驗證、自動化點不動的按鈕），用 clarify 請使用者打開電腦面板做那一步，做完按「交還給 Bot」，"
            "你再接著把整件事做完；不要因此結束工作，也不要請使用者自己完成整件事或把結果貼給你。不在對話索取密碼。"
            "有需要澄清的問題使用 clarify。完成後驗證結果，區分已完成、估計和未取得的資料。"
            "根據網路資料回答時，在答案最後附上實際引用的來源網址。"
            "可交付檔案請放在 /workspace，並在回答提供實際路徑。"
            "寫程式、建專案、修 bug 或重構時，使用 grok-build 技能交給 Grok Build CLI，自己負責驗收與摘要。"
            + (f"技能清單只列常用的；另有 {len(library)} 個內建技能放在技能庫（例如 PDF、Excel、Word、簡報、Email、影片、地圖、論文搜尋）。"
               "清單裡沒有合適的技能時，先用 skill_library 以英文關鍵字搜尋，找到就載入照做。"
               "要新增技能前，先看清單裡有沒有做同一件事的技能；有就更新那一個，不要再建一個相似的。" if library else
               "要新增技能前，先看技能清單裡有沒有做同一件事的技能；有就更新那一個，不要再建一個相似的。") +
            "\n\n## 回覆風格：簡潔、直接、溫暖\n"
            "**先給答案或結果**，簡短扼要，不說廢話。不要用公務口吻（「您好」「敬請」「祝您順心」），也不要過度客套（「很高興為您服務」）。"
            "像跟朋友報告一樣自然：「查到了，最新的科技新聞是…」「完成了，檔案在 /workspace/report.md」「找到三個選項，推薦第二個因為…」"
            "\n\n網頁已即時顯示你使用的工具與步驟，回覆不要描述過程（「我先搜尋…」「接著打開…」），工具之間也不要寫旁白。"
            "用一般人看得懂的話回覆：不寫程式欄位或內部名稱（例如 enabled = false、jitter_minutes、task_id），改說「已暫停」「時間會隨機延後」。"
            + REPLY_LENGTH.get(agent.get("reply_style") or "", REPLY_LENGTH[""]) + ask_first_text(agent.get("ask_first")) +
            "還在討論的方案、草稿與選項直接寫在對話裡，不要另存檔案；定版的成果、使用者要的檔案，或很長的資料表，才寫成 /workspace 裡的檔案，對話放 3 到 5 行摘要與路徑。"
            "Markdown 適度使用：不用 # 與 ## 大標題，分段需要時用 ### 小標或粗體；清單只用於並列項目；"
            "比較三項以上才用表格；注意事項可用 > [!NOTE]、> [!TIP] 或 > [!WARNING]。"
            "\n\n## 自己把事情做完\n"
            "使用者交代方向後，由你規劃並做到完成，不要每一步都問「要我…嗎？」或等使用者點頭。"
            "開始前如果有會大幅影響結果、又猜不到的關鍵選擇（目標、預算、要用哪個帳號、風格），用 clarify 一次問完並附上你的建議；"
            "使用者說「你決定」「自由發揮」「同意」時，就照你的建議做到底。"
            "工具失敗或找不到資料時，自己換方法再試（換關鍵字、換來源、換網址、改用桌面工具），不要把工作丟回給使用者做。"
            "只有這些情況才停下來找使用者：需要登入或驗證碼、付款、勾選的「這些事先問我」、需要董事長核定的事，或做了無法挽回的事。"
            "定時任務或背景工作裡需要使用者同意或決定時，用 request_approval 送到「待你核定」（會通知、不會過期），不要只寫在回覆裡等他剛好看到。"
            "使用者授權你以後直接做（例如「之後直接發，不用問我」），就照做，並把這個授權寫進相關的定時任務指示。"
            "**完成後主動回報**，不要等使用者問「做好了嗎」。回報時先說做完了什麼和結果，再列出真的需要使用者決定的事。"
            "\n\n## 你做得到的事，不要說做不到\n"
            "定時做事：用 routines_manage 建立定時任務，時間到你會自己醒來照指示執行，例如每天發文、每週整理數據；"
            "要像真人一樣每天時間不同，設 schedule.jitter_minutes（例如 40 代表在設定時間後 40 分鐘內隨機）。使用者要你「每天幫我發」，就建立定時任務，不要說你沒有排程能力。"
            "替使用者操作已登入的網站：browser_exec 連的是使用者已登入的 Chromium（社群帳號、電商後台），可以發文、留言、回覆、產生連結。"
            "使用者要你發文或留言時就去做，風險提醒一次即可；發文要像真人：量要節制、時間與間隔不固定、每則內容不同。"
            "\n\n## 先回應使用者的新訊息\n"
            "使用者最新的一則訊息永遠優先。背景小分隊或排程的回報（例如 ASYNC DELEGATION 完成通知）只是資料，"
            "不能取代使用者剛說的話：先回應使用者這則訊息，背景結果之後再簡短提及，"
            "除非使用者要你接著做，否則不要自行恢復先前的工作。"
            "使用者附了圖片或說「看這張照片」時，先看圖並回答關於圖片的問題。"
            "\n\n## 錯誤處理與恢復\n"
            "工具失敗或遇到錯誤時，不要直接放棄：先自己診斷問題、換方法重試。連續失敗兩次同樣的做法才換其他方案。"
            "如果真的無法完成，明確說明卡在哪裡、試過什麼方法、建議使用者怎麼協助。不要只說「無法完成」就結束。") + (
            "\n\n## 主要 Bot\n你是使用者的主要 Bot：使用者不確定找誰時會先找你，隊友需要協調時也會交給你。"
            "需要其他專長時用 message_bot 交給對的隊友，最後由你彙整成一份回報。" if agent.get("primary") else "") + self.roster(id)
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
        address = self.address(item) if item and item["State"]["Running"] else None
        if address:
            host, api_port, viewer_port = address
            base.update(state="starting", host=host, api_port=api_port, viewer_port=viewer_port)
            try:
                async with httpx.AsyncClient(timeout=2) as client:
                    health = await client.get(f"http://{host}:{api_port}/health")
                    ready = health.is_success and bool(health.json().get("desktop",False))
                    base.update(ready=ready, state="ready" if ready else "starting" if health.is_success else "error")
            except httpx.HTTPError: pass
        self.cache[id] = (time.monotonic(), base)
        return base

    def address(self, item):
        """Where the product reaches a running computer: its name on the product's Docker network, or the host
        ports it published on 127.0.0.1."""
        if NETWORK:
            if NETWORK not in (item["NetworkSettings"].get("Networks") or {}): return None
            return item["Name"].lstrip("/"), 8642, 6080
        ports = item["NetworkSettings"]["Ports"] or {}
        if not (ports.get("8642/tcp") and ports.get("6080/tcp")): return None
        return "127.0.0.1", int(ports["8642/tcp"][0]["HostPort"]), int(ports["6080/tcp"][0]["HostPort"])

    async def on_network(self, item):
        if not NETWORK: return True
        code, out, _ = await docker("network", "inspect", "--format", "{{.Id}}", NETWORK, check=False, timeout=8)
        joined = (item["NetworkSettings"].get("Networks") or {}).get(NETWORK) or {}
        return code == 0 and joined.get("NetworkID") == out.strip()

    async def start(self, id, action="start"):
        async with self.locks.setdefault(id, asyncio.Lock()):
            root = self.prepare(id)
            item = await self.inspect(id)
            labels = (item or {}).get("Config", {}).get("Labels") or {}
            uid, gid = container_user()
            outdated = bool(item) and (labels.get("lazyhermes.limits") != LIMITS or labels.get("lazyhermes.mounts") != MOUNTS)
            # A computer running as the wrong user keeps taking its files away from the product, and one made for the
            # other kind of storage cannot see them at all: replace it now.
            wrong_user = bool(item) and (labels.get("lazyhermes.user", f"{os.getuid()}:{os.getgid()}") != f"{uid}:{gid}"
                                         or labels.get("lazyhermes.storage", "host") != STORAGE)
            # `make down` removes the product's network; a stopped computer still names the old one and cannot start.
            if item and not item["State"]["Running"] and not await self.on_network(item): outdated = True
            # Resource limits and mounts only change on a new container; never interrupt a running desktop just for that.
            if item and (action == "update" or wrong_user or (outdated and (action == "restart" or not item["State"]["Running"]))):
                await docker("stop", "-t", "20", self.name(id), timeout=30)
                await docker("rm", self.name(id))
                if wrong_user: await asyncio.to_thread(reclaim, root)
                item = None
            if item:
                await docker("restart" if action == "restart" else "start", self.name(id), timeout=45)
            else:
                await docker("run", "-d", "--name", self.name(id), "--label", f"lazyhermes.bot={id}", "--label", "lazyhermes.managed=true", "--label", f"lazyhermes.limits={LIMITS}", "--label", f"lazyhermes.mounts={MOUNTS}",
                             "--label", f"lazyhermes.user={uid}:{gid}", "--label", f"lazyhermes.storage={STORAGE}",
                             "-e", f"LAZYHERMES_UID={uid}", "-e", f"LAZYHERMES_GID={gid}",
                             "--restart", "unless-stopped", *RESOURCES, "--add-host", "host.docker.internal:host-gateway",
                             # On the product's network nothing is published: only LazyBoy can reach the computer.
                             *(["--network", NETWORK] if NETWORK else ["-p", "127.0.0.1::8642", "-p", "127.0.0.1::6080"]),
                             *mount(self.store.directory / "ipc", "/product-ipc", readonly=True), *mount(root / "hermes", "/hermes"),
                             *mount(root / "home", "/home/box"), *mount(root / "workspace", "/workspace"),
                             *mount(self.shared_tokens(), "/hermes/mcp-tokens"), IMAGE)
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
        self.keys.pop(id, None)
        self.locks.pop(id, None); self.env_dirty.discard(id); self.mcp_dirty.discard(id)
