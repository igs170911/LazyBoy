# LazyBoy 安裝指引（AI Agent 適用）

這份文件讓 AI coding agent（Claude Code、Codex CLI、Cursor、Gemini CLI 等）在使用者的 Linux 主機上，從零安裝並啟動 LazyBoy。請依序執行，每一步都附有驗證方式，驗證通過才進行下一步。手動安裝也適用同一套步驟。

使用者可以把下面這段話直接交給自己的 agent：

```text
請依照 https://github.com/igs170911/LazyBoy/blob/master/INSTALL.md 在這台電腦安裝 LazyBoy。
需要 sudo 或安裝系統套件前先問我，完成後告訴我網址和下一步。
```

## 執行原則

1. **系統層級的變更先徵得同意。** 安裝系統套件、使用 `sudo`、把使用者加入 `docker` 群組之前，先列出要做的事並取得使用者同意。
2. **不要索取密碼或金鑰。** 需要 sudo 密碼時，請使用者自己在終端機執行指令。模型的 API key 由使用者在網頁「設定 → 模型」自行填入，不要在對話中要求，也不要寫進檔案或指令。
3. **長時間的指令請在背景執行。** 第一次安裝會建置約 4.6 GB 的 Docker 映像，通常超過 10 分鐘。請放到背景執行並追蹤輸出，或把逾時拉長；不要因為逾時就重複啟動同一個安裝。
4. **不要刪除資料。** `.data/` 存放所有 Bot、對話與登入資料，安裝過程不需要動它。也不要執行 `docker system prune` 這類會影響其他專案的清理指令。
5. **每一步都要驗證。** 驗證失敗時先查「疑難排解」，不要略過驗證繼續往下做。

## 系統需求

| 項目 | 需求 |
| --- | --- |
| 作業系統 | Linux x86_64，使用 apt（Debian、Ubuntu）、dnf（Fedora）、pacman（Arch）或 zypper（openSUSE）的發行版。ARM64、macOS 與 Windows（WSL2）尚未驗證 |
| 權限 | 能使用 `sudo`（只有安裝缺少的系統套件時需要） |
| 磁碟空間 | 至少 20 GB 可用：Hermes 原始碼約 1.5 GB，Bot 映像約 4.6 GB，另外還有建置快取與 Bot 資料 |
| 記憶體 | 建議 8 GB 以上。每個 Bot 的電腦最多使用 2 CPU、3 GB 記憶體 |
| 網路 | 可連到 GitHub、PyPI、npm registry、nodejs.org、Docker Hub、ghcr.io 與 Debian 鏡像站 |

其他需要的軟體（git、make、python3、curl、Docker Engine 與 Buildx、Node.js 20 以上、uv）都可以由 `make install` 自動安裝。

## 步驟 1：取得原始碼

```bash
git clone https://github.com/igs170911/LazyBoy.git ~/LazyBoy
cd ~/LazyBoy
```

> [!IMPORTANT]
> 專案的絕對路徑請控制在 80 個字元（bytes）以內，例如 `~/LazyBoy`。服務會在 `<專案>/.data/ipc/product.sock` 建立 Unix socket，而 Linux 規定 socket 路徑不能超過 108 bytes。中文字在路徑中每字佔 3 bytes。

**驗證**：`git -C ~/LazyBoy log -1 --oneline` 會印出一個 commit。

## 步驟 2：檢查缺少的套件

```bash
./scripts/install.sh --check
```

這個指令只檢查，不會改動任何東西。它會列出缺少的項目以及安裝方式；全部都已安裝時顯示「需要的系統套件都已安裝。」，結束代碼為 0。

清單中的系統套件（基本工具、Docker、啟動 Docker、加入 `docker` 群組）需要 sudo。Node.js 與 uv 會安裝在 `~/.local`，不需要 sudo。

- **有需要 sudo 的項目**：把清單告訴使用者並取得同意。agent 通常無法輸入 sudo 密碼，請使用者在自己的終端機執行下一步的 `make install`（在 Claude Code 中可以輸入 `! make install`）。使用者同意，而且這台主機的 sudo 不需要密碼時，agent 也可以自己執行。
- **沒有需要 sudo 的項目**：直接進行下一步。

## 步驟 3：安裝

```bash
make install
```

沒有 `make` 時，改用 `./scripts/install.sh`；兩者完全相同。這個指令依序執行：

1. 安裝缺少的系統套件、Docker、Node.js 與 uv（步驟 2 列出的項目）
2. 下載 Hermes Agent，並切到 `HERMES_VERSION` 指定的固定版本（`vendor/hermes-agent`）
3. 依鎖檔安裝 Python 依賴（`uv sync --frozen`），安裝並建置網頁前端（`npm ci`、`npm run build`）
4. 測試各 Debian 鏡像站的速度，用最快的那一個建置 Bot 電腦的 Docker 映像：`lazyhermes-desktop:local`，以及以它為基礎的 `lazyhermes-bot:local`
5. 檢查所有安裝產物

第一次執行需要下載大量套件，通常超過 10 分鐘，請放在背景執行並追蹤輸出。重新執行是安全的：已安裝的套件會略過，原始碼會沿用，Docker 也會使用建置快取。

**驗證**：輸出最後是 `安裝完成。執行 make start，再打開 http://localhost:8790`。另外執行：

```bash
python3 scripts/doctor.py --ready
```

除了標示 `WARN` 的選用項目以外，每一行都要是 `PASS`，包含 `Pinned Hermes`、`.venv/bin/python`、`web/dist/index.html` 與兩個映像。

> [!NOTE]
> 安裝過程如果把使用者加入了 `docker` 群組，輸出會提醒重新登入。請告訴使用者：先登出再登入（或在終端機執行 `newgrp docker`），之後的 `make start` 才能使用 Docker。

## 步驟 4：啟動服務

```bash
make start
```

服務會在背景執行，日誌寫在 `.run/server.log`。第一次啟動時會建立 `.data/`，產生登入碼，並建立一個名為「LazyBoy」的 Bot。

**驗證**：輸出 `LazyBoy 已啟動：http://localhost:8790`，接著執行：

```bash
curl -fsS http://127.0.0.1:8790/api/health
```

回應是 JSON，其中包含 `"ok":true` 與 `"engine":"hermes"`。

8790 port 已被占用時，改用其他 port，例如 `LAZYHERMES_PORT=8800 make start`，之後每次啟動都要帶同一個值。以下步驟中的 `8790` 也要跟著換掉。

## 步驟 5：確認 Bot 的電腦能啟動

這一步會啟動預設 Bot 的 Docker 電腦，確認映像可以正常運作。不會呼叫任何模型，也不會產生費用。

```bash
id=$(curl -fsS http://127.0.0.1:8790/api/agents | python3 -c 'import sys, json; print(json.load(sys.stdin)["agents"][0]["id"])')
curl -fsS -X POST -H 'content-type: application/json' -d '{"action":"start"}' "http://127.0.0.1:8790/api/agents/$id/computer"
```

**驗證**：90 秒內回應 JSON，其中 `"ready": true`。電腦啟動後會保持執行，這是正常狀態。

## 步驟 6：交給使用者

告訴使用者以下事項：

1. **網址**：在這台主機上打開 <http://localhost:8790>。
2. **設定模型**（只能由使用者操作）：打開「設定 → 模型」，選擇供應商、貼上 API key、選擇模型。出現「✓ 可以使用」就完成了。支援 OpenRouter、Anthropic、OpenAI、Gemini、DeepSeek 等 40 多個供應商，也可以使用任何 OpenAI 相容的服務。
3. **其他裝置**（手機、筆電）：連到 `http://<這台主機的 IP>:8790`，第一次需要輸入登入碼。登入碼請使用者自己在專案資料夾執行 `make get-access-token` 取得，不要貼在對話裡。跨網路使用請放在 HTTPS 反向代理後面。
4. **日常操作**：`make start` 啟動、`make stop` 停止。停止服務會中斷進行中的工作，但檔案、對話與登入都會保留。

## 疑難排解

| 症狀 | 處理方式 |
| --- | --- |
| `sudo: a terminal is required` 或安裝程式提示需要 sudo 密碼 | 請使用者在自己的終端機執行 `make install` |
| `permission denied while trying to connect to the Docker daemon socket` | 目前的登入還沒有 `docker` 群組。請使用者登出再登入（或執行 `newgrp docker`），再重新執行指令 |
| 建置時出現 `--chmod`、`--mount` 或 BuildKit 相關錯誤 | 缺少 Docker Buildx 或 Docker 太舊。執行 `make install` 補裝 Buildx，或把 Docker 升級到 23 以上 |
| Debian 套件下載很慢，或選到的鏡像站出錯 | 用 `DEBIAN_MIRROR` 指定鏡像站並略過測速，例如 `DEBIAN_MIRROR=http://deb.debian.org/debian make install` |
| `OSError: AF_UNIX path too long` | 專案路徑太長。把專案移到較短的路徑，或用 `LAZYHERMES_DATA_DIR` 指定較短的資料目錄 |
| `服務尚未啟動，請查看 .run/server.log` | 查看 `tail -n 50 .run/server.log`。常見原因是 port 被占用（`Address already in use`） |
| `這個資料目錄已有 LazyBoy 服務在執行` | 服務已在執行。要重新啟動請執行 `make stop`，再 `make start` |
| Bot 電腦一直無法就緒 | 查看 `docker logs --tail 50 lazyhermes-bot-<Bot id>`，以及主機的可用記憶體 |
| 其他裝置連不上 | 確認主機防火牆允許 TCP 8790，而且只在信任的網路開放 |
| 新開的終端機找不到 `node` 或 `uv` | 它們安裝在 `~/.local/bin`。把 `export PATH="$HOME/.local/bin:$PATH"` 加進 shell 設定檔 |

## 另一種方式：在 Docker 中執行

使用者要求「只裝 Docker」，或主機上的工具與權限問題一直無法解決時，改用這個方式。它取代步驟 2 到 5；主機只需要 git 與 Docker Engine 26 以上（含 Buildx 與 Compose）。

```bash
cd ~/LazyBoy
docker version --format '{{.Server.Version}} {{.Server.Os}}/{{.Server.Arch}}'   # 需要 26 以上，linux/amd64 或 linux/arm64
make build   # 第一行會印出建置平台；第一次通常超過 10 分鐘，請在背景執行
make up
```

**成功的標準：** `make up` 最後印出「LazyBoy 已啟動」與含 `#token=` 的網址。再確認 Bot 的電腦能啟動（容器名稱預設是 `lazyboy`）：

```bash
token=$(docker exec lazyboy cat /data/access-token)
curl -fsS -c /tmp/lazyboy-cookie -H 'content-type: application/json' -d "{\"token\":\"$token\"}" http://127.0.0.1:8790/api/auth/login
id=$(curl -fsS -b /tmp/lazyboy-cookie http://127.0.0.1:8790/api/agents | python3 -c 'import json,sys; print(json.load(sys.stdin)["agents"][0]["id"])')
curl -fsS -b /tmp/lazyboy-cookie -X POST -H 'content-type: application/json' -d '{"action":"start"}' "http://127.0.0.1:8790/api/agents/$id/computer"
rm -f /tmp/lazyboy-cookie
```

回應中有 `"ready": true` 就完成了。把 `make up` 印出的網址交給使用者，並提醒網址含登入碼，不要外流。

- 登入碼只交給使用者，不要寫進檔案或 commit；需要時用 `make docker-token` 重新取得。
- 主機原本已用 `make install` 安裝過：先 `make stop`，再 `make docker-import`，然後 `make up`。匯入只讀取 `.data/`，不會改動它。
- `make up` 回報 Docker 版本太舊時，請使用者升級 Docker，不要改用其他方式繞過。

## 更新既有的安裝

```bash
cd ~/LazyBoy
git pull
make install
make stop && make start
```

在 Docker 中執行時，改為 `git pull && make build && make up`。

已經存在的 Bot 電腦會繼續使用舊映像。請在網頁「設定 → 更新」，或在 Bot 的電腦選單選「更新電腦環境」，套用新的映像。

## 解除安裝

> [!WARNING]
> 以下指令會刪除所有 Bot 與資料，執行前一定要取得使用者確認，並提醒先備份 `.data/`。

```bash
cd ~/LazyBoy
make stop
docker ps -aq --filter label=lazyhermes.managed=true | xargs -r docker rm -f
docker image rm lazyhermes-bot:local lazyhermes-desktop:local
cd ~ && rm -rf ~/LazyBoy
```

`make install` 安裝的 Docker、Node.js 與 uv 不會被移除；`~/.local/lib/nodejs` 與 `~/.local/bin` 中的 `node`、`npm`、`npx`、`uv` 可以視需要自行刪除。
