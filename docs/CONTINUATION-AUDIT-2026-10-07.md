# LazyBoyHermes 續作檢查

2026-10-08 已補入當時缺少的自動路由、分類器、安裝檢查、跨日回歸與比較入口。後端回歸 79 項通過，Docker／Chromium 部署驗收因環境限制待重跑；目前狀態見 [流程補齊紀錄](WORKFLOW-COMPLETION-2026-10-08.md)。以下保留 2026-10-07 的實測證據。

日期：2026-10-07。基準為 LazyBoy2 的產品說明、架構研究中的六項網頁整合驗收，以及兩個專案的實際前端/API 程式。

中斷時確實尚未完成：已有前端及部分 Hermes/容器轉接，但缺少完整產品入口、啟停腳本及驗收；續作期間也找出多個前後端協定和恢復問題。現在主要功能已接通，以下區分實測證據、修復及仍未涵蓋的原版能力。

## 本次補齊與修復

| 範圍 | 已完成的處理 |
| --- | --- |
| 產品入口 | 使用單一 server.main:app，保留原 React 網頁，HTTP 與 Worker Unix socket 由同一程序管理；開發及測試預設連到 :8790。 |
| 任務及歷史 | 巢狀交易完整回滾；持久 request_id 去重；同 Bot 的 admission/補充序列化；修復 session 快取及正常 WebSocket 關閉後未標示中斷；終結後忽略遲到 delta。 |
| 停止及恢復 | 區分 stopping 與 terminal/cancelled；停止準備中及佇列工作；產品重啟標示未知結果中斷、不重播。改用正常 native session resume，關閉 Hermes crash auto-continue。 |
| Worker 租約 | 獨占 Bot 的 Hermes home；記錄上一個 Worker 的 PID/namespace，啟動時僅退休該 Worker 的失效會話/壓縮租約；保留其他 owner 和無法辨識的租約。處理 Docker 重用 PID namespace 的情況。 |
| 人類互動 | 保留 native clarify qid、多題獨立答案及秘密輸入；僅明確核准才允許；秘密答案不進產品對話、task 或事件。 |
| 檔案與錄影 | 支援前端 video base64、影片及截圖 manifest；將示範說明及取樣畫面送入原生模型；工作成果依真正檔案變化產生；修復 /workspace/... 絕對路徑預覽；拒絕目錄越界、Bot 跨讀及外部 symlink。 |
| 技能與記憶 | 操作原生 SKILL.md；啟用 native memory 工具；實際技能寫入後才通知 UI；停用/刪除反映到執行中的 Hermes；清除聊天保留工作檔、技能、記憶與瀏覽器 profile。 |
| 群組及 MCP | 取消群組成員會釋放原成員；支援 UI 的 remove；同端點留白保存原憑證；native test 結果映射成 UI 狀態及缺失欄位。修復 product MCP 子程序缺少 HERMES_HOME，讓原生排程工具可用。 |
| 分享及匯入 | 清除 headers/env、URL、啟動 args 及已知私密值；匯入 connector 預設停用；同 import key 重送回原 Bot、異內容拒絕；失敗不留下半個 Bot。 |
| 模型 | 任務保存實際 provider/model/protocol/推理強度；下一任務採用新設定，包含同模型的圖片能力變更；網頁可新增供應商、手動模型及選擇 API 介面；三種介面均完成原生工具驗收。預先安裝 Anthropic SDK，轉接接受 Messages 的 x-api-key，並傳遞版本及 beta 標頭。 |
| 容器與啟動 | 與主機相同 UID/GID，修復一般使用者仍寫 /root/.config 的瀏覽器回歸；修復重複啟動清空現有 service.lock PID；提供固定版本 bootstrap、前景/背景啟動與停止說明。 |

## 驗證證據

- uv run pytest -q：41 項通過，使用臨時資料，不執行正式 Bot 或付費模型。
- TypeScript、Vite 建置、資產預算與 gzip/Brotli 內容檢查通過。
- 分享、電腦恢復及 transcript 的桌面/手機 Playwright 回歸：23 通過、1 個既有手機案例略過。使用隔離的靜態 build，API 為測試 fixture。
- tests/native_continuation_flow.py 使用真正 Docker、固定版本且未修改核心的 Hermes、本機決定式模型；已實測工具寫檔/讀回、同一 Chromium 的 DOM 與桌面觀察、noVNC、多題澄清、技能建立/讀回/分享/停用/刪除、私有記憶、模型切換、工作中補充、真實停止、原生 MCP 排程只觸發一次、不同 Bot 隔離及日常重啟後 cookie/檔案/記憶持久化。
- 工作途中 Worker 強制重啟的完整回歸已通過：正確標成 interrupted，下一個明確任務沿用對話繼續，未知副作用指令不重播。完整原生驗收 39 個檢查點通過，證據見 [原生驗收資料](benchmarks/native-continuation-2026-10-07.json)。
- tests/native_media_flow.py 通過：執行中的 Bot 更改圖片能力後，上傳圖片內容及 high 推理設定送入下一次原生模型請求；錄影截圖及說明也送達模型；桌面觀察回傳 PNG，依元素觀察點擊並導航同一個 Chromium；自訂 stdio MCP 保存、測試及移除。證據見 [媒體驗收資料](benchmarks/native-media-2026-10-07.json)。
- tests/native_protocols_flow.py 通過：真正網頁設定新供應商、手動模型及 API 介面；Responses/Messages 各自接收圖片、執行寫檔和讀回，下載成果正確，同一個 Worker 可切換介面。證據見 [介面驗收資料](benchmarks/native-protocols-2026-10-07.json)。
- 真正網頁/API 的桌面及手機 Playwright 驗收 8 項通過、無略過；另在主 Bot 檢查兩種畫面的真實 noVNC 連線，JavaScript 錯誤為零。證據見 [網頁驗收資料](benchmarks/web-acceptance-2026-10-07.json)。

可重跑指令：

```bash
./scripts/check.sh
./scripts/check.sh --native
npm run test:e2e --prefix web
```

## 尚未等同原版的範圍

1. 2026-10-08 已加入 semantic/Laya 與 chat/work 自動路由、工具後升級及備援，routing health 回傳實際分類器狀態。同一組分流模型須使用相同 API 介面，工具發現及批准由 Hermes 負責；新增原生部署驗收尚未完成。
2. Chat Completions、Responses/Messages 已使用真正 SDK、Hermes 工具及本機決定式供應商完成流程；正式模型工作使用 OpenCode Go/DeepSeek。不同商業供應商的模型能力、額外參數及外部 MCP 認證方式，仍需依該服務實測，不能由介面相容推論全部服務可用。
3. 乾淨主機的完整安裝、不同機器資源限制、完整繼承前端測試集、長時間/跨日排程及大型錄影學習，尚未全部實測。
4. 2026-10-08 已提供相同條件比較器，包含交錯順序、每案例至少五次、瀏覽器準備與實際成果下載核對；實際模型、任務、帳號及瀏覽器條件的比較尚未執行。整合通過不能宣稱更快，也不能宣稱能完成所有網站任務。

2026-10-07 的主服務及主 Bot 曾載入當日修復，三組原生驗收在同一個映像通過。更新後正式模型任務 `d2e1b246-4309-41ec-beae-6b56e1f07f6e` 成功讀回原有檔案並操作瀏覽器。2026-10-08 的新增程式未在此受限工作階段重啟服務載入。舊 LazyBoy2 的服務與電腦沒有參與新版驗收。
