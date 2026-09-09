<div align="center">

<img src="./apps/web/public/lazyboy-icon.png" width="96" height="96" alt="LazyBoy" />

[English](./README.md) · **繁體中文**

# LazyBoy

**給 AI 一台電腦，讓它替你動手做事。**

自架的 AI Agent 工作空間。用文字或語音交代任務，即時觀看桌面操作，隨時接回控制權。

[快速開始](#快速開始) · [功能](#功能) · [架構與流程圖](./docs/architecture.md) · [部署指南](./docs/operations.md) · [開發指南](./docs/development.md)

</div>

![LazyBoy 群組對話：訊息由其中一個 Agent 接手回覆，右側是它自己的即時桌面](./docs/readme-hero.png)

LazyBoy 讓 Agent 在 Docker 裡使用自己的 Linux 桌面，操作瀏覽器、終端與檔案。你可以建立不同的 Agent、在群組中協作，把示範整理成技能，再安排定時執行。

目前為早期版本 `0.1.0`，提供桌面與手機瀏覽器介面；模型金鑰由你自行設定。

## 功能

- **持續的工作空間**：每個 Agent 有自己的對話、工作紀錄與可設定的長期記憶。
- **真的能操作電腦**：開網頁、使用終端、整理檔案、操作圖形介面，過程可即時觀看。
- **附件直接丟進對話**：訊息可帶檔案或圖片，Agent 能在自己的桌面開啟，放在收件匣的複本會自動到期清除。
- **隨時人工接管**：在同一個桌面完成登入、驗證或手動調整，再交回 Agent。
- **登入帳密進保險庫**：帳密加密存在 Agent 自己的保險庫，遇到登入頁由它填寫，密碼不會經過模型。
- **多 Agent 與群組**：支援 Team 共用電腦與 Private 獨立電腦模式；群組裡 @誰就由誰回，沒點名時只叫醒工作內容相關的那個，不會全部出動。
- **示範教學與排程**：把操作示範整理成技能，使用 cron 安排重複工作。
- **自選模型與工具**：支援 xAI、OpenCode Go、OpenAI 相容端點，以及 MCP 與檔案技能。
- **語音通話**：設定語音服務後，可以透過通話與 Agent 互動。
- **手機操作**：可收合聊天側欄，遠端桌面提供鍵盤、觸控板、右鍵、拖曳與捲動控制。
- **英文與繁體中文**：可在介面切換語言。

## 快速開始

需要 Docker 與 Compose、Git、Make、Python 3，以及一組支援的模型 API 金鑰。macOS 可使用 Docker Desktop 或 OrbStack；Linux 可使用 Docker Engine。

取得本專案原始碼後，在專案根目錄執行：

```bash
make env
```

編輯產生的 `.env`，填入 `XAI_API_KEY`、`OPENCODE_GO_API_KEY` 或 `OPENAI_API_KEY` 其中之一。初始化工具會產生登入與服務所需的金鑰，重新執行時保留既有設定。

```bash
make up
make health
```

開啟 **[http://127.0.0.1:3101](http://127.0.0.1:3101)**，用 `.env` 裡的 `LAZYBOY_APP_TOKEN` 登入，建立 Agent 並選擇模型。

第一次會從原始碼建置 API 與 Linux 桌面映像，所需時間較長。容器內已包含建置環境，完整 Docker 部署不需要在主機安裝 Rust 或 Node.js。

可以先試一個具體任務：

> 開啟我指定的網站，整理頁面重點，將結果存成工作區裡的 Markdown 檔案。

服務狀態與日誌：

```bash
make ps
make logs
make down  # 停止服務，保留 PostgreSQL 資料
```

資源限制、環境變數、HTTPS 與容器內 sudo 設定，請見 [部署與操作](./docs/operations.md)。

## 在手機上使用

手機與桌面使用同一個 Web 介面。API 預設只綁 `127.0.0.1`，請在 `.env` 設定 `LAZYBOY_BIND_IP=0.0.0.0`（或指定網卡位址）並重建 api 容器，區網裡的手機才連得到；綁非 loopback 時登入 token 需至少 32 字元。接著用手機瀏覽器開啟該位址——手機上的 `127.0.0.1` 只代表手機本身，不能拿來連另一台電腦。

聊天側欄可點外側空白處收合。操作遠端桌面時，可切換直接點選與觸控板模式，使用工具列叫出鍵盤、按右鍵或拖曳。對外提供服務時請設定 HTTPS，詳見 [部署指南](./docs/operations.md#安全模型)。

## 技術組成

- **前端**：React 19、TypeScript、Vite、noVNC
- **後端**：Rust 2024、Axum、Tokio
- **資料**：PostgreSQL、pgvector、SQLx
- **桌面**：Docker、Debian、XFCE、Chromium、Xvfb
- **電腦控制**：Cua Driver（X11、AT-SPI、Chromium）
- **擴充**：MCP、檔案技能、示範 playbook

流程圖、控制權交接、元件職責與電腦生命週期狀態機，集中在 **[架構與流程](./docs/architecture.md)**。原有的 **[互動流程圖](./docs/workflow.html)** 也保留；下載後用瀏覽器開啟即可操作。

## 本機開發

除了 Docker，需安裝支援 Rust 2024 edition 的 Rust 工具鏈，以及 Node.js／npm。

```bash
make dev
make dev-supervisor  # 終端 1
make dev-api         # 終端 2
```

前端熱更新使用另一個終端：

```bash
cd apps/web
npm install
npm run dev
```

開啟 [http://127.0.0.1:5173](http://127.0.0.1:5173)。測試指令與專案目錄說明請見 [開發指南](./docs/development.md)。

## 文件

| 文件 | 內容 |
| --- | --- |
| [架構與流程](./docs/architecture.md) | 任務流程圖、系統架構、電腦生命週期狀態機 |
| [互動流程圖](./docs/workflow.html) | 可縮放、搜尋的 HTML 圖表；下載後開啟 |
| [部署與操作](./docs/operations.md) | 資源、環境變數、安全設定、網站驗證、sudo |
| [AI 使用體驗](./docs/agent-experience.md) | 輪次政策、持久終端機、聊天即時推送 |
| [hermes-agent 比較](./docs/hermes-agent-cua-review.md) | Cua 操作流暢度：與 hermes-agent 對照 |
| [開發指南](./docs/development.md) | 本機開發、檢查與測試、目錄結構 |
| [設定範例](./.env.example) | 環境變數與預設值 |

## 資料

對話、記憶、瀏覽器設定檔與加密憑證保存在自架主機。使用外部模型時，任務所需的提示詞、工具結果與截圖仍可能傳送給該模型供應商。

---

<div align="center">

**danielwang** <img src="https://flagcdn.com/w20/tw.png" width="20" alt="Taiwan" />

[igs170911@gmail.com](mailto:igs170911@gmail.com)

<a href="https://www.buymeacoffee.com/daniel.wang.1993"><img src="https://cdn.buymeacoffee.com/buttons/v2/default-yellow.png" alt="請我喝杯咖啡" height="50" /></a>

[Apache License 2.0](./LICENSE)

</div>
