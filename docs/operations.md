# 部署與操作

[← 回到 README](../README.zh-TW.md)

## 安全模型

目前程式碼包含下列防護：

- API session token 與 Supervisor token 分離。
- Supervisor 只在內部 Compose network，並使用 `no-new-privileges`、唯讀 root filesystem 與 capability drop。
- 每台電腦有 CPU、RAM、PID 上限；預設 2 CPU、2 GB、2048 PID。
- API 預設只綁定 `127.0.0.1:3101`。
- 憑證以獨立 `LAZYBOY_VAULT_KEY` 加密，輪替登入 token 時不應更換此 key。
- 已保存登入只接受 HTTPS、精確或合法子網域匹配，不對相似惡意網域填入。
- Markdown 連結限制為 HTTP(S)、`mailto:`、`tel:` 與頁內錨點。
- 日誌有大小與檔案數上限；診斷資料有可設定的保留週期。

部署注意事項：

1. 對區網或網際網路開放前，先放在 HTTPS reverse proxy 後方，並設定 `LAZYBOY_SECURE_COOKIE=true`。
2. 不要把 Supervisor `:7091` 對外發布，也不要將 Docker socket 掛進 Agent 電腦。
3. `LAZYBOY_APP_TOKEN`、`SANDBOX_SUPERVISOR_TOKEN`、`LAZYBOY_VAULT_KEY` 必須使用不同的高熵值。
4. 模型仍可能看見任務所需的網頁內容與截圖；密碼、token 與高敏感資料不要放進提示詞。
5. 簡單的 Cloudflare 連線驗證可嘗試一次正常點擊；未通過、其他 CAPTCHA 與 2FA 交由使用者接管。

## 硬體與資源

以下為容量規劃起點，並非效能測試結果；實際需求取決於同時啟動的電腦與瀏覽器工作量。

| 項目 | 最低可執行 | 建議 |
| --- | --- | --- |
| CPU | 4 核 | 8 核以上 |
| RAM | 8 GB，單台電腦 | 16 GB 以上 |
| 磁碟 | 約 15 GB | Docker 至少保留 30 GB |
| GPU | 不需要 | 模型預設走外部 API |
| 作業系統 | macOS / Linux | Linux 可選配 LXCFS 顯示容器內 cgroup 配額 |

每台電腦的預設限制可由 `LAZYBOY_COMPUTER_CPUS`、`LAZYBOY_COMPUTER_MEMORY_MB`、`LAZYBOY_COMPUTER_PIDS` 調整。瀏覽器分頁的心跳會維持熱機；沒有工作且約 10 分鐘無人觀看時暫停，持續休眠約 6 小時後停止。

## 設定

`make env` 會以 `.env.example` 為基礎建立 `.env`，並保留既有金鑰。

| 變數 | 用途 | 預設 |
| --- | --- | --- |
| `XAI_API_KEY` | xAI 模型 | 空 |
| `OPENCODE_GO_API_KEY` | OpenCode Go 模型 | 空 |
| `OPENAI_API_KEY` | OpenAI 相容端點 | 空 |
| `LAZYBOY_APP_TOKEN` | Web 登入 token | 必填 |
| `SANDBOX_SUPERVISOR_TOKEN` | API ↔ Supervisor 驗證 | 必填 |
| `LAZYBOY_VAULT_KEY` | 憑證庫加密 key | 必填且必須保持穩定 |
| `LAZYBOY_BIND_IP` | 主機監聽位址 | `127.0.0.1` |
| `LAZYBOY_SECURE_COOKIE` | HTTPS-only cookie | `false` |
| `LAZYBOY_SCREEN_NETWORK` | API ↔ 電腦 noVNC 的 Docker 內網名稱 | `lazyboy_screen` |
| `LAZYBOY_SCREEN_UPSTREAM` | 只取代 loopback 位址的 noVNC 除錯用 host，容器名稱不受影響 | `127.0.0.1` |
| `LAZYBOY_COMPUTER_CPUS` | 每台電腦 CPU | `2` |
| `LAZYBOY_COMPUTER_MEMORY_MB` | 每台電腦記憶體 | `2048` |
| `LAZYBOY_COMPUTER_PIDS` | 每台電腦 PID 上限 | `2048` |
| `LAZYBOY_COMPUTER_SUDO` | 容器內免密碼 sudo；重建桌面容器後生效 | `false` |
| `LAZYBOY_COMPUTER_DRIVER` | 只支援 `cua`。舊後端已移除；更新映像後需重建桌面容器。 | `cua` |
| `LAZYBOY_MEMORY_ENABLED` | 長期記憶 | `true` |

完整清單與保留政策請見 [`.env.example`](../.env.example)。

### 從其他裝置連線

`LAZYBOY_BIND_IP` 決定主機在哪個位址發布 `:3101`，改完重建 api 容器生效：

```bash
LAZYBOY_BIND_IP=0.0.0.0      # 區網所有介面可連
LAZYBOY_BIND_IP=10.0.33.1    # 只開放指定網卡
docker compose up -d api
```

- 綁非 loopback 時 `LAZYBOY_APP_TOKEN` 必須至少 32 字元，否則 api 拒絕啟動。
- Origin 檢查比對 `Origin` 與 `Host`：直接開 `http://<主機IP>:3101` 可正常使用，從其他網域嵌入會被 `403 cross-origin request rejected` 擋下。
- 純 HTTP 下 session cookie 以明碼走區網；長期或跨網際網路使用請放到 HTTPS reverse proxy 後面，並設定 `LAZYBOY_SECURE_COOKIE=true`。
- 暫時性跨網存取建議維持 `127.0.0.1` 綁定改用隧道：`ssh -L 3101:127.0.0.1:3101 <host>`。
- 桌面 noVNC 走 `LAZYBOY_SCREEN_NETWORK` 這條 internal 網路，不佔主機埠；同機跑多組 LazyBoy 時請為每組取不同名稱，compose 與 supervisor 會共用同一個值。

## 執行記錄與錯誤診斷

任務執行中的每一步會寫進 `run_activity`：模型每一輪在想什麼、哪個動作成功或
失敗、花了幾秒、現在是第幾輪。這屬於診斷資料，不是對話內容，因此可以被清理。

**看即時記錄。** 在對話框把滑鼠移到「思考中」的頭像上（或點一下釘住、用 `Tab`
聚焦），會浮出即時記錄面板：

- 標題列顯示「第 {n}/{limit} 輪」、已運行時間與現在在做什麼；goal 模式沒有輪次
  上限時只顯示目前輪次，不會假裝有上限。
- 記錄由舊到新排列並自動捲到底，包含模型回合、動作成敗與逾時、重試第幾次，以及
  停下來等你的原因，最後停在哪裡一目了然。
- 「複製記錄」可把整份純文字記錄貼給別人除錯；錯誤原文以等寬字型原樣顯示。
- 面板只在開啟時每 1.5 秒增量抓取 `GET /api/runs/{id}/activity`，run 結束
  （完成／失敗／取消）後停止輪詢，不開就完全不打 API。

**出錯時說什麼。** 任務失敗會在對話框出現錯誤卡，而不是一行紅字：

- 一句人話講清楚原因與下一步，例如「模型不認得這個 API 金鑰。到「設定 →
  模型」重新貼一次金鑰，再按重試。」
- 按鈕依錯誤類型給：`重試`（從中斷的地方接著做，保留 checkpoint，不重複已成功的
  步驟）、`開模型設定`、`打開它的畫面`。金鑰錯誤不會只給重試，電腦消失也不會叫
  你去看設定。
- `ⓘ` 直接展開同一個即時記錄面板看詳細 log；錯誤代碼與原文都在裡面。
- 重試走 `POST /api/runs/{id}/retry`，只對失敗或已取消的 run 生效；同一台電腦還有
  別的工作在跑時會被擋下，並明確告訴你是因為還有別的工作。

**保留多久。** `LAZYBOY_RUN_ACTIVITY_RETENTION_DAYS` 控制記錄保留天數，預設 7 天，
每小時清理一次。想留更久的除錯軌跡就調大，在意資料庫體積就調小；run 本身被
`LAZYBOY_RUN_RETENTION_DAYS` 清掉時，它的記錄一併消失。

## 任務跑多久：輪次政策

一輪就是一次模型呼叫。任務**沒有輪數額度**：電腦工作會一直做到模型提出驗證、或明白說卡在哪裡。
停下來只有三種原因，由輕到重：

| 層 | 觸發 | 結果 |
| --- | --- | --- |
| 提示 | 同一個動作連做 3 次、同一個錯誤連錯 3 次、40 輪沒有新的成功、到檢查點（第 60 輪起每 120 輪）、跑超過 `LAZYBOY_RUN_SOFT_MINUTES` | 系統在下一輪前塞一句具體提醒，任務**繼續**；一次最多提示 8 次，不會變噪音 |
| 暫停（`loop_detected`） | 同一個**會改變狀態**的動作連做 6 次或整輪累計 12 次、同一個動作連錯 8 次、連續 14 個動作都失敗、150 輪沒有任何新的成功 | 任務停在目前畫面，訊息裡附上「哪個動作重複了幾次」，按「繼續」就從中斷點接下去 |
| 保險絲（`budget_exhausted`） | 超過 `LAZYBOY_RUN_CAP_TURNS` 輪或 `LAZYBOY_RUN_HARD_MINUTES` 分鐘 | 代表迴圈失控，是故障不是成績；照樣可續跑，但要去看執行記錄 |

- 「看畫面、等待、讀檔」不算鬼打牆：觀察與輪詢不會被當成重複動作，等待長 build、下載、佇列都是正常的。
- 計時從**本次嘗試**開始算：任務停下來等你回話三天，續跑時時鐘歸零。
- 聊天（沒有電腦工作的純對話）仍然是 4 輪上限，那是避免模型在閒聊中燒掉額度，跟任務長度無關。

微調方式寫在 `.env`（改完重建 api 容器生效）：

```bash
LAZYBOY_RUN_SOFT_TURNS=60    # 第一次自我檢查點的輪數
LAZYBOY_RUN_SOFT_EVERY=120   # 之後每隔多少輪再檢查一次
LAZYBOY_RUN_CAP_TURNS=1000   # 輪數保險絲
LAZYBOY_RUN_SOFT_MINUTES=75  # 自我檢查的時間點
LAZYBOY_RUN_HARD_MINUTES=240 # 時間保險絲
```

真的很久的工作（大計畫、批量資料處理）不該塞在一個 run 裡，改用排程工作分段跑，比較容易驗證也比較省 token。

## 容器內終端機

Agent 透過 Cua 操作 VNC 上的真實終端機。`session` 指定持續使用的視窗；`command` 輸入命令，
省略則只查看畫面。`keys: "C-c"` 可中斷前景命令，`reset: true` 重新建立乾淨的登入 shell。
`cwd` 只有明確指定時才改變既有終端機的工作目錄。

`wait_ms` 預設 1000 毫秒、最多 10000 毫秒。回傳的是截圖，等待時間到不代表命令完成；
請看提示字元與畫面上的結果，長工作可以稍後再次查看。長輸出可用 Cua 捲動。
直接在共用 VNC 點選同一個視窗即可觀看與接管，不需要額外開 tmux 工作階段。

## 網站連線驗證

AI 遇到可辨識的 Cloudflare 連線驗證頁時，會先取得最新桌面截圖，再使用
`connection_check` 嘗試點擊可見的驗證框一次。工具沿用現有瀏覽器，最多進行三次
間隔五秒的結果檢查（不含瀏覽器工具本身的執行時間）。驗證頁消失後仍須確認目標
內容已載入；未通過則顯示接管提示。其他圖形／音訊驗證和 2FA 仍交由使用者處理。

測試：更新 API 服務後，開新任務要求 AI「開啟 Dcard 並讀取指定文章；若有簡單的
連線驗證框，使用 connection_check 嘗試一次」。可在任務狀態看到「嘗試連線驗證並
確認結果」。此功能不保證網站放行，也不會偽造驗證結果。

## 容器內使用 sudo

在 `.env` 設定 `LAZYBOY_COMPUTER_SUDO=true`，執行
`docker compose up -d --no-deps supervisor`，再於網頁選擇「重新啟動電腦」。
這會重建桌面容器，讓 `lazyboy` 使用者能執行免密碼 `sudo`；可用
`sudo -n id -u` 檢查，預期輸出 `0`。既有容器只暫停／恢復不會套用此設定。
重新啟動前請儲存工作；家目錄保留，容器系統層自行安裝的套件需重新安裝。

## Postgres collation 版本不符

`pgvector/pgvector:pg16` 是浮動 tag。重新 `docker compose pull` 之後，容器內的 glibc
版本可能和 `pgdata` 卷建立時不同，Postgres 會在使用每個資料庫時抱怨：

```text
WARNING:  database "template1" has a collation version mismatch
DETAIL:   The database was created using collation version 2.41, but the operating system provides version 2.36.
```

此時 `CREATE DATABASE` 直接被拒（`ERROR: template database "template1" has a collation
version mismatch`），凡是需要建立資料庫的測試都會失敗：`cargo test` 裡的 `#[sqlx::test]`
表現在連線逾時（`PoolTimedOut`），`tests/retention.test.py`、`tests/run-resume.test.py`
則在 `CREATE DATABASE` 就掛掉。

原地修好，資料不遺失（會重建索引，資料量大時安排在離峰）：

```bash
docker compose stop api
make pg-collation
docker compose up -d api
```

它對 `template1`、`postgres`、`lazyboy` 各跑一次 `REINDEX DATABASE` 與
`ALTER DATABASE ... REFRESH COLLATION VERSION`。collation provider 是 libc，排序規則
真的可能跟著 glibc 變，所以先重建索引再更新記錄的版號，不要只改版號。想避免重複發生，
把 Postgres 映像改成固定 digest，讓同一個資料卷不會被不同 glibc 開起來。
