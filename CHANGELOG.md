# Changelog

All notable changes to LazyBoy are documented here.

## [v0.1.0-alpha] - 2026-09-09

First public alpha. Self-hosted AI agent workspace: each agent gets an isolated Linux desktop, you watch it live, and you can take over at any time.

This is a source release. There is no pre-built binary; `make up` builds the API, supervisor, and desktop images from this tree.

### Highlights

- **A real computer per agent** — Debian + XFCE + Chromium in Docker. Browser, terminal, files, and GUI, with a live noVNC view.
- **Cua desktop driver** — clicks, typing, windows, clipboard, and the visible terminal go through Cua (`cua-driver-rs` v0.23.2). The old AT-SPI / CDP path is gone.
- **Chat that carries work** — text, images, and file attachments; the agent opens them on its own desktop. Inbox copies expire on their own.
- **Take over, then hand back** — sign in, pass a check, or nudge the same desktop by hand. Saved logins live in an encrypted vault and never pass through the model.
- **Several agents and groups** — shared Team computers or private desktops. `@name` wakes one agent; otherwise a short router picks who should answer.
- **Teach by demo, then schedule** — turn a walkthrough into a skill; cron runs it again.
- **Your models and tools** — xAI, OpenCode Go, OpenAI-compatible endpoints, MCP, and file skills.
- **Voice calls** — talk to the agent after you enable a voice provider.
- **Desktop and phone UI** — English and Traditional Chinese. On a phone: collapsible chat, keyboard, trackpad, right-click, drag, and scroll.
- **Runs that finish the job** — no hard turn quota on real work. The agent has to verify against the current screen before it stops; looping is paused for you instead of being cut off. Hover the thinking avatar for the live trace; a failed run can be retried.
- **Memory and hygiene** — optional long-term memory (pgvector), room-scoped recall, and retention jobs for traces, recordings, and checkpoints.

### Install

You need Docker and Compose, Git, Make, Python 3, and an API key for a supported model.

```bash
git clone https://github.com/igs170911/LazyBoy.git
cd LazyBoy
git checkout v0.1.0-alpha
make env
```

Edit `.env` and set one of `XAI_API_KEY`, `OPENCODE_GO_API_KEY`, or `OPENAI_API_KEY`. Then:

```bash
make up
make health
```

Open [http://127.0.0.1:3101](http://127.0.0.1:3101) and sign in with `LAZYBOY_APP_TOKEN`. The first run builds images from source and takes a while.

### Alpha caveats

- Early software. Expect rough edges in the desktop driver, group routing, and long-running tasks.
- The API binds `127.0.0.1` by default. For a phone on your network set `LAZYBOY_BIND_IP=0.0.0.0` (token must be at least 32 characters). Put it behind HTTPS before you expose it.
- Do not publish supervisor port `7091` or mount the host Docker socket into an agent computer.
- You bring the model API key. Screenshots and page contents can still reach the model; keep passwords in the vault, not in the prompt.

See [Operations](./docs/operations.md) for resource limits, environment variables, and the security model.

### 繁體中文

第一個公開 alpha。自架 AI Agent 工作空間：每個 Agent 有自己的隔離 Linux 桌面，你可以即時觀看，也可以隨時接手。

這是原始碼發行版，沒有預先編好的執行檔；`make up` 會從此樹狀目錄建出 API、supervisor 與桌面映像。

**重點**

- **每個 Agent 一台電腦** — Docker 裡的 Debian + XFCE + Chromium。瀏覽器、終端、檔案與圖形介面，畫面用 noVNC 即時看。
- **Cua 桌面驅動** — 點擊、打字、視窗、剪貼簿與可見終端都走 Cua（`cua-driver-rs` v0.23.2）。舊的 AT-SPI / CDP 路徑已移除。
- **對話裡就能交工作** — 文字、圖片、檔案附件；Agent 在自己的桌面開啟。收件匣複本會到期清除。
- **隨時接管再交回** — 同一桌面登入、過驗證或手動調整。帳密放加密保險庫，不會經過模型。
- **多 Agent 與群組** — Team 共用電腦或 Private 獨立桌面。`@名字` 只叫醒一個人；沒點名時由短路由決定誰回。
- **示範教學與排程** — 把操作示範收成技能，再用 cron 重複執行。
- **自選模型與工具** — xAI、OpenCode Go、OpenAI 相容端點、MCP、檔案技能。
- **語音通話** — 設定語音服務後可以直接通話。
- **桌面與手機介面** — 英文與繁體中文。手機可收合聊天側欄，遠端桌面有鍵盤、觸控板、右鍵、拖曳與捲動。
- **任務以做完為準** — 正式工作不再用固定輪數腰斬。Agent 要對著當下畫面自我驗證才准停；鬼打牆會暫停等你，而不是直接切斷。滑過思考頭像可看即時紀錄，失敗的 run 可以重試。
- **記憶與清理** — 可選的長期記憶（pgvector）、依房間範圍回想，以及紀錄、錄影、checkpoint 的保留週期。

**安裝**

需要 Docker 與 Compose、Git、Make、Python 3，以及一組支援的模型 API 金鑰。

```bash
git clone https://github.com/igs170911/LazyBoy.git
cd LazyBoy
git checkout v0.1.0-alpha
make env
```

編輯 `.env`，設定 `XAI_API_KEY`、`OPENCODE_GO_API_KEY` 或 `OPENAI_API_KEY` 其中之一，然後 `make up` 與 `make health`。打開 [http://127.0.0.1:3101](http://127.0.0.1:3101)，用 `LAZYBOY_APP_TOKEN` 登入。第一次會從原始碼建映像，需要一段時間。

**Alpha 注意事項**

- 早期軟體。桌面驅動、群組路由、長任務都還可能不穩。
- API 預設只綁 `127.0.0.1`。要給區網手機用請設 `LAZYBOY_BIND_IP=0.0.0.0`（token 至少 32 字）。對外之前請放在 HTTPS 後面。
- 不要把 supervisor `:7091` 對外，也不要把主機 Docker socket 掛進 Agent 電腦。
- 模型金鑰自己準備。截圖與頁面內容仍可能進模型；密碼放保險庫，不要寫在提示詞裡。

資源上限、環境變數與安全模型見 [部署指南](./docs/operations.md)。
