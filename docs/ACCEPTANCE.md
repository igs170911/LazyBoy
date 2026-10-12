# 整合驗收紀錄

2026-10-08 更新：自動模型分流、分類器接入、安裝檢查及比較入口已補入程式；79 項後端回歸與建置通過。下表是 2026-10-07 的部署驗收，新增的 Docker／Chromium 實測受本次環境阻擋，詳見 [流程補齊紀錄](WORKFLOW-COMPLETION-2026-10-08.md)。

日期：2026-10-07。驗收對象為新資料夾中的網頁、產品服務與每個 Bot 的原生 Hermes 電腦。這份文件區分實際模型工作、決定式模型整合測試，以及使用 API fixture 的舊版回歸。

服務已啟動於 **http://localhost:8790**。主 Bot 與三組原生整合驗收都使用映像 `sha256:35c307d16cae1d3ba4c8eb63a4ced182f0b1823289d4e69d31bddaf7a27f0208`。Hermes 固定於 `05eecbcd972c8737ebc7722ea08aab47fb538043`，上游核心沒有修改。

| 要求 | 目前證據 |
| --- | --- |
| 新專案分開保存 | `/home/daniel/Projects/LazyBoyHermes`，沒有修改原 LazyBoy2 產品程式 |
| 沿用完整網頁 | React 原前端已接上產品 API；TypeScript、Vite 和壓縮資產檢查通過 |
| 真正的 Hermes 執行 | 固定 commit 的上游核心；真實 DeepSeek 模型任務 `806bc395-de51-49c4-9c89-1396edbd1bb2` 建立並讀回 `hermes-smoke.txt`，成果可下載 |
| 真正的自己的電腦 | 任務 `61f0b33c-8c12-48ba-b35c-0b5485520d52` 成功讀桌面元素、按 ctrl+l、輸入網址、Return，再用 browser_exec 確認同一頁面 |
| 網頁電腦接手 | Playwright 實際打開 noVNC、建立 RFB 連線；父頁面狀態顯示 Running，原有剪貼簿與行動控制轉接已帶入 |
| 多題與複選互動 | 任務 `d69c3c6f-b2d1-4722-a0c3-b74014692b0c` 的兩題卡片在桌面與手機呈現，重新整理後仍存在；從網頁選藍色及 PDF、Markdown，原生 Hermes 讀到正確回答並寫入檔案 |
| 真實停止 | 任務 `2d532cf2-5369-499a-a88f-c3eb10c06503` 前景指令每秒寫入檔案；停止先回 stopping，之後 terminal/cancelled。結束後六秒內容仍為相同 2 bytes |
| 產品回歸檢查 | `uv run pytest -q`：41 項通過；涵蓋交易回滾、重送、並行補充、路徑與 symlink 隔離、秘密不進產品紀錄、排程重送、分享匯入、MCP 秘密保存、錄影 payload、Worker 租約恢復及 Messages 原生驗證方式 |
| 不依賴付費模型的完整迴圈 | `tests/native_continuation_flow.py`：39 個檢查點通過；真正 Docker 與未修改核心的 Hermes，本機決定式模型，33 次前景、36 次總模型請求。詳見 [原生驗收資料](benchmarks/native-continuation-2026-10-07.json) |
| 圖片、錄影、推理及自訂 MCP | `tests/native_media_flow.py` 通過：在同一個執行中的 Bot 開啟圖片能力後，PNG 原始內容與 high 推理設定到達下一個原生模型請求；錄影截圖與說明也送入模型。桌面 MCP 回傳真正 PNG，依觀察到的元素操作同一個 Chromium；自訂 stdio MCP 保存、測試、移除通過。詳見 [媒體驗收資料](benchmarks/native-media-2026-10-07.json) |
| Responses 與 Messages | `tests/native_protocols_flow.py` 通過：從真正網頁新增供應商、手動模型、選介面及圖片能力；同一個 Bot 切換兩種介面，每種收到圖片、執行 terminal 寫檔、read_file 讀回，並下載核對成果。詳見 [介面驗收資料](benchmarks/native-protocols-2026-10-07.json) |
| 真正網頁及 API | `npm run test:e2e --prefix web`：桌面/手機共 8 項通過、沒有略過；建立 Bot、選模型、原生技能、分享匯入、上傳、例行工作、遠端登入、原生 MCP 測試皆使用真正產品 API。驗收 Bot 已清理。詳見 [網頁驗收資料](benchmarks/web-acceptance-2026-10-07.json) |
| 繼承前端回歸 | 分享、電腦恢復及 transcript 的桌面/手機 Playwright 測試：23 通過、1 個既有手機案例略過；這組使用隔離 build 和 API fixture，不取代真正網頁驗收 |
| 一般使用者身分下的持久檔案 | 新映像實測檔案為主機 UID；終端、瀏覽器及原生技能可正常寫入各自持久目錄 |
| 工作中補充 | 在真正的原生長時間指令中送入補充，產生要求的 CSV 並讀回；重送補充不新增第二個任務或對話 |
| 兩個獨立 Bot | 兩台實際容器的 home/workspace/Hermes 掛載、工作檔、私有記憶及 Chromium cookie 隔離通過 |
| 原生技能、MCP 與排程 | 原生 skill_manage/skill_view 建立及讀回，UI 分享、停用、刪除通過；原生 memory 寫入並在清除對話後載入；內建 desktop/product MCP 通過，過期一次性排程只觸發一次。外部 MCP 的各種認證方式仍需另行驗證 |
| 停止後再啟動與持久化 | 瀏覽器 cookie、工作檔與記憶在電腦重啟後保留；下個原生任務使用新的模型及供應商設定；重複啟動不清空現有 service.lock 的 PID |
| 工作途中 Worker 強制重啟 | 任務正確標示 interrupted；下一個明確任務沿用歷史繼續；未完成且可能有副作用的指令不會自動重播 |

主服務已重啟、主 Bot 已更新至上述映像。更新後實際模型任務 `d2e1b246-4309-41ec-beae-6b56e1f07f6e` 使用 `opencode-go / deepseek-v4.1-flash`，透過原生工具讀回原有 22-byte 檔案並開啟工作瀏覽器，結束為 `terminal/answer`。桌面與手機實際呈現此對話、建立 noVNC RFB 連線，頁面沒有 JavaScript 錯誤。畫面見 [桌面](screenshots/desktop-ready.png)、[手機](screenshots/mobile-ready.png)。

研究文件的六項網頁整合要求均有對應證據：持久重送與恢復、補充及真實停止、網頁互動及接手、Worker 中斷處理、Bot 電腦隔離、附件/成果/學習/分享及單一排程器。2026-10-07 部署驗收時模型使用全域或 Bot 指定值；2026-10-08 已加入自動路由及比較流程，新增部署驗收尚未完成。尚未進行相同條件的速度/品質比較，不能將整合成功解讀為速度改善或所有網站任務都能成功。其他驗證範圍見 [續作檢查](CONTINUATION-AUDIT-2026-10-07.md)。
