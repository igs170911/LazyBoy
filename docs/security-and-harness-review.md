# 安全、Harness 與 Docker 檢查

日期：2026-09-05。檢查第一方 Rust workspace、前端、控制腳本及容器設定；保留工作目錄原有修改。這是程式與依賴檢查，不是完整滲透測試；reference 第三方專案未逐行稽核。

## 已修正的安全問題

| 風險 | 問題與修正 | 主要位置 |
| --- | --- | --- |
| 高 | 桌面原本持有 supervisor 主 token；改為依 home key 衍生的 HMAC token，管理路由驗證容器歸屬 | crates/supervisor/src/docker.rs、main.rs |
| 高 | 可提供任意 home 路徑及透過 symlink 越界；限定 DATA_DIR/homes/key，附件改用 cap-std 目錄能力讀寫 | supervisor/docker.rs、api/attachments.rs |
| 高 | 已儲存密碼可能填入不相符網站；現在強制指定 hostname 與 HTTPS，不自動送出；DOM 觀察遮蔽密碼類欄位 | api/tools.rs、control/cdp.py |
| 高 | 桌面提供的 JavaScript 可能在 API 同源執行；noVNC 靜態程式改由 API 可信映像提供 | api/screen_proxy.rs、image/api/Dockerfile |
| 中 | 固定登入 cookie 無法個別撤銷；改為隨機 session、伺服器期限及登出撤銷，加入 Origin／Fetch Metadata 與本機 Host 檢查 | api/auth.rs |
| 中 | MCP 子程序繼承服務秘密；改為環境白名單及明確設定 | api/mcp.rs |
| 中 | 工具輸出可能進入記錄；移除輸出摘錄；附件隨機儲存名稱避免覆寫 | api/runs.rs、attachments.rs |

Session 目前放在單一 API 記憶體，重啟失效；不適用多副本共用登入。已建立的 WebSocket 不會因 cookie 撤銷立即關閉。MCP 仍是同 UID 的可信子程序，環境白名單不等於作業系統沙箱。

## Harness 與電腦控制

- 修正 observe 對 POST-only 端點誤用 GET，消除正常路徑每次失敗再 fallback。
- 移除有副作用操作失敗後的自動重播；執行前落盤 toolsStarted，完成批次保存上下文 checkpoint。worker 在不確定操作是否執行的狀態中斷，會標記失敗而非重做。
- 模型暫時失敗採有限次重試與退避；各工具及程序有期限與輸出上限。Checkpoint 移除截圖並限制 1 MiB；超限保留不確定狀態以避免盲目重播。這不是 exactly-once 保證。
- 使用 websocket-client 處理 CDP framing、控制訊框與 timeout；限制 Chromium 除錯介面為 loopback，移除 wildcard Origin。
- DOM snapshot 使用每次唯一 selector，降低舊觀察誤點新元素風險；點擊前檢查可見、啟用、遮擋，動作後等待有限畫面更新。
- 指標／視窗觀察並行；終端剪貼簿使用正確快捷鍵，確認 X11 文字一致後再貼上。
- 排程以交易與 SKIP LOCKED 避免多 worker 重複派發，入列與下次時間一併提交。Unix 星期轉換已有回歸測試。DOM 與 DOW 同時受限的 cron 明確拒絕，避免不同 cron 引擎的 OR/AND 語意差異。

這些改動減少多餘往返與重播風險，尚未進行真實桌面延遲 benchmark，沒有速度倍數保證。DOM 優先、需要時使用桌面操作仍是本專案適合的路徑；沒有為了換框架重寫整個控制層。

## Docker 改善與升級

已加入 cargo/npm 建置 cache、cargo --locked、npm ci、直接 MCP 套件版本固定、API 非 root、capabilities 限縮、健康檢查、restart/init、程序與日誌上限。資料庫／控制網路獨立；預設只在 127.0.0.1:3101 開放 API，Postgres 不對主機映射。開發資料庫使用 make postgres 的額外 Compose 設定。

新安裝執行 make env，產生四組獨立秘密與隨機資料庫密碼，檔案權限 0600。既有 .env 不會被覆寫。

既有安裝更新時：

1. 備份資料庫、data 與 .env。此次僅建置映像，沒有重啟你的正式服務、旋轉秘密或刪除既有桌面。
2. 保留原本 vault key。若之前未設定 LAZYBOY_VAULT_KEY，先把原本 LAZYBOY_APP_TOKEN 的值保存為 LAZYBOY_VAULT_KEY，才能旋轉 app token；否則舊密碼可能無法解密。
3. 舊桌面曾取得 supervisor 主 token，更新時應更換 LAZYBOY_SUPERVISOR_TOKEN 並重建所有舊桌面容器（保留 home 資料）。新版 controlVersion 會使舊容器在重新 provision 時重建。
4. API 改為 UID 1000。檢查既有 API 資料與快取目錄是否可由 UID 1000 存取；只調整確定需要的目錄，勿遞迴改動所有使用者 home。HOST_DATA_DIR 必須對應正確的主機資料目錄。
5. 既有 Postgres volume 不會因改 .env 自動換密碼；密碼輪替須同時更新資料庫角色與連線設定。API 重啟後重新登入。

Supervisor 仍掌握 Docker socket，可控制 Docker 主機；cap_drop 不能消除此權限。較強隔離方案是專用 Docker daemon／VM。桌面 Chromium 既有 --no-sandbox 與可信 MCP 的執行邊界仍需納入威脅模型。基底映像尚未固定 digest，未做完整 OS image CVE 掃描；直接套件固定版本不代表所有下載資產都可完全重現。

## 驗證與剩餘告警

- Rust workspace：114 個測試通過，含隔離 PostgreSQL 測試；前端 Node 5 個、Python 3 個測試通過。
- 前端 typecheck、production build、程式碼 diff 空白檢查（不含 README 的 Markdown 換行空白）、Compose config 驗證通過；三個 Docker 映像實際建置成功。
- 隔離 API smoke test：未登入拒絕、跨站登入拒絕、合法登入可存取 API、可信 noVNC asset 可用、登出後舊 cookie 被拒絕；均使用最後建置的映像驗證。
- npm audit：0。cargo-audit：RUSTSEC-2023-0071（rsa 0.9.10，無修補版本）仍在 lockfile，但目前啟用依賴樹 cargo tree -i rsa 無結果；不能把未使用的 lockfile 告警說成已移除。
- paste 1.0.15 有停止維護告警 RUSTSEC-2024-0436，由 fastembed／影像相關上游引入，仍需追蹤替代版本。
- 未完成瀏覽器視覺驗收、真實 VNC 剪貼簿端到端、故障注入／負載測試。Lottie eval 提醒仍存在。

## 方法參考

- [Docker 安全與 daemon 信任邊界](https://docs.docker.com/engine/security/)、[Docker socket 保護](https://docs.docker.com/engine/security/protect-access/)、[建置快取](https://docs.docker.com/build/cache/optimize/)
- [Playwright actionability](https://playwright.dev/docs/actionability)：參考動作前狀態檢查原則，並未導入 Playwright runtime。
- [長時間 agent harness](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents)：持久化進度與恢復的設計參考。
- [CDP 協定](https://chromedevtools.github.io/devtools-protocol/)、[websocket-client 用法](https://websocket-client.readthedocs.io/en/latest/examples.html)
- [RSA advisory](https://rustsec.org/advisories/RUSTSEC-2023-0071.html)、[paste advisory](https://rustsec.org/advisories/RUSTSEC-2024-0436.html)
