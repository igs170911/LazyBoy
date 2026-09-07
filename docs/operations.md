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
