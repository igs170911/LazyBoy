# 缺少流程的補齊紀錄

已補入產品程式的流程：聊天／工作模型分流、工具後升級、備援、本機 Semantic Router／Laya、安裝前後檢查、跨日回歸及相同模型的比較入口。上游 Hermes 執行核心未修改。此文件區分實作完成與實際部署驗收，2026-10-07 的原生證據保留為歷史紀錄。

## 模型與分類流程

在「設定 → 模型 → 對話與工作分流」選聊天、工作及最多三個備援模型。各角色可指定推理強度與等待上限。明確複雜工作、桌面、續作、例行工作及圖片使用工作角色；其他前景工作先用聊天角色。原生工具開始執行後，下一次模型請求升級到工作模型；工具結果也能判斷升級，不依賴 WebSocket 通知一定先送達。

429、暫時性 5xx、連線失敗或第一個串流位元組之前的讀取錯誤，依序使用工作模型及備援；400／401 等非暫時性錯誤不切換。串流已開始後不重播。等待模型標頭或首個位元組時，使用者停止也會取消請求並阻止備援。

每個任務固定當時的供應商設定及憑證，工作中修改設定不會換掉該任務的金鑰。任務記錄每次前景模型請求嘗試的實際模型、供應商、角色及介面；秘密與原始請求不進事件。記憶／標題等無工具的輔助呼叫使用原生設定，維持原有流程。Bot 自己指定的模型不套用全域路由。

單組聊天、工作與備援必須使用相同 API 介面，因為執行中的原生 SDK 使用固定介面。Chat Completions、Responses、Messages 各自支援此流程；不同 Bot 可選不同介面。跨模型保留工具 call ID 與配對結果，移除不適用的 opaque reasoning／compaction 及舊模型推理參數；Messages 推理參數依固定版本 Hermes 的模型規則處理。原生工具發現與權限確認保持 Hermes 負責，不套用原 Rust 工具清單模式。

本機分類器使用 LazyBoy2 的服務程式與鎖定依賴，獨立部署於 `8778`，不啟停 LazyBoy2 的分類器：

```bash
./scripts/routing.sh start
./scripts/routing.sh status
./scripts/routing.sh stop
```

服務初次啟動下載及暖機模型。在分流進階設定填本機 URL，啟用 Semantic Router／Laya。預設只記錄分類；取消「僅記錄分類結果」後才採用超過門檻的結果。分類 deadline 預設 250 ms，連線錯誤有 30 秒 cooldown；不確定或不可用時交給工作模型。分類不批准操作或停用工具。可直接連到既有相容分類器，例如 LazyBoy2 的 `8768`。

## 安裝與重跑

```bash
./scripts/bootstrap.sh --check-only
./scripts/bootstrap.sh --with-routing
python3 scripts/doctor.py --ready
./scripts/check.sh
./scripts/check.sh --native
```

安裝檢查涵蓋 Git、uv、Node 20 以上、npm、Docker daemon、Compose、固定 Hermes 版本、前端及映像產物。`python3 scripts/doctor.py --static --ready` 可只做靜態檢查。標準原生驗收現在包含 `tests/native_routing_flow.py`，用真正 SDK 與工具驗證聊天升級、429 備援及下載成果，全部使用本機模型和獨立 Bot。

前端路由驗收不需要啟動產品或測試伺服器，透過瀏覽器攔截載入真正 production build：

```bash
cd web
npx playwright test e2e/hermes/hermes-routing.spec.ts
```

新版網頁偵測到舊後端尚未支援路由時，會停用儲存控制並提示重新啟動服務，避免顯示已儲存但後端未採用。沒有工作執行時可用既有 `scripts/stop.sh`、`scripts/start-background.sh` 載入新版。

## 速度與成果比較

兩邊先設定相同供應商 ID、模型、API 基底網址、介面和明確推理強度；比較器拒絕不相符的組合。預覽不執行模型：

```bash
uv run python scripts/compare.py --provider PROVIDER_ID --model MODEL_ID
```

核對兩邊使用相同模型帳號後，執行實際比較：

```bash
uv run python scripts/compare.py --provider PROVIDER_ID --model MODEL_ID \
  --live --same-account --samples 5 --output .run/comparison.json
```

它建立自己的臨時 Bot，先讓兩台電腦就緒，再交錯執行，每個案例至少五次。每次使用不同成果檔名，防止把上次留下的檔案當成這次成功；核對實際下載內容，取消和失敗不混進成功延遲。每次開始新對話、保留比較電腦；第一個 Worker 樣本與後續樣本分開。比較結束刪除自己的 Bot 與登入資料。

`--cases cases.json` 可提供自己的純文字成果案例，欄位為 `id/prompt/path/expected`；`path` 是工作目錄內的相對成果路徑，須出現在 prompt 中。登入案例標示 `requires_login: true`，並加 `--prepare-browser`：比較器會先顯示兩個比較 Bot 的名稱，等待你在網頁電腦完成相同登入與頁面準備後才開始。模型帳號相同是操作者聲明；網站帳號、CPU/記憶體限制與頁面條件仍需核對，不能由模型端點相同推論。

報告只輸出案例 ID、成功、時間及可取得的計數／token，不包含原始 prompt、帳號或憑證。Hermes 的 model_requests 是前景 HTTP 嘗試，Rust 的 requests 是原有 budget 計數，分別保存；未知欄位用 null，不填零。小樣本只報 warm median，不宣稱 p95 或所有網站任務的速度改善。

## 2026-10-08 實際驗證

- 後端 79 項通過，包含新路由的三種協定、憑證及端點、快照、真實產品佇列到 native session 的模型選擇、工具通知延遲、未配對結果與 bookkeeping 不升級、跨模型 compaction 與推理參數、開始前／開始後串流錯誤、停止等待、分類 deadline／cooldown、週期恢復及比較器隔離。使用 ASGI/MockTransport，不使用付費 API。
- 靜態安裝檢查、Python 編譯、shell 語法、TypeScript、Vite、資產預算及壓縮內容檢查通過。
- 一個既有真正 WebSocket 測試未在本次執行：環境禁止 socket 操作。檔案回應測試使用臨時 timer plugin 維持受限環境的 asyncio 喚醒，未改產品執行方式。
- 新前端桌面／手機測試均在 Chromium 啟動前被環境阻擋：`setsockopt: Operation not permitted`；沒有宣稱通過。
- Docker daemon 存取被拒絕，尚未執行新增的原生路由流程、重新跑完整原生驗收、部署分類器、重啟正式服務或在乾淨主機重裝。實際模型／登入案例的速度比較也尚未執行。

目前可以確認程式與離線回歸完成；新容器、瀏覽器及實際比較需要在可存取 Docker、Chromium 與網路的工作階段完成。證據見 [本次驗證狀態](benchmarks/workflow-completion-2026-10-08.json)。

## 2026-10-08 前端改版與對話串

- 前端重寫為分割工作區：對話在中間，「電腦／檔案」面板在右側，手機改為分頁；工作進度在對話中串流，Hermes 提問固定在輸入框上方。舊版只存在瀏覽器的本機群組聊天已移除。
- 回覆 Markdown 改用 GFM、CJK 粗體修正與保留單一換行，程式碼區塊有語法上色與複製按鈕；Markdown 引擎延遲載入，資產預算檢查通過。
- 同一 Bot 支援多個對話串，每串一個原生 Hermes session（`session.create`／`resume`），Hermes 的 `session.title` 自動命名，使用者改名優先。一臺電腦一次只做一件事：其他串的訊息排隊，補充與提問回答只送進正在工作的那一串。標題與清單預覽會去除 Markdown 標記。
- 修正：群組移動到不存在的群組回 404、改群組名稱不再清空成員、`make stop` 在 SSE 連線開著時不再卡住（graceful shutdown 5 秒）、串流中重新載入不再遺失已收到的文字。
- 驗證：`uv run pytest -q` 118 通過；`npm run build` 與資產檢查通過；`npx playwright test e2e/hermes` 桌面＋手機 36 通過（含真正產品 API 的 `product.spec.ts`）。正式服務重啟後實測：新對話串取得獨立 Hermes session，主要對話工作中時新串排隊、完成後才執行並回覆 Markdown，主要對話紀錄不受影響。

## 2026-10-08 電腦畫面（VNC）速度與穩定度

量測用暫時 Bot 與 headless Chromium，經產品服務的 `/novnc` 轉接，量完已刪除。

| 項目 | 修改前 | 修改後 |
| --- | --- | --- |
| 轉接是否再壓縮（permessage-deflate） | 是，每張畫面在服務主執行緒壓縮 | 否 |
| 原始畫面轉接（1280×720×10 張） | 62 ms／張 | 4 ms／張 |
| 終端機畫面持續變動時，產品服務 CPU | 21% | 8% |
| 同時間 API 最慢回應 | 80–94 ms | 14–41 ms |
| Bot 電腦 CPU 吃滿時，按鍵到畫面出現 | 中位數 148 ms | 中位數 65 ms |
| 畫面連線中斷 | 等 8 秒後重新載入整頁 | 0.6–2 秒在原頁面重連 |
| 面板切到檔案分頁 | 持續串流，Bot 電腦多花約 16% CPU 編碼 | 15 秒後暫停，切回 0.1 秒恢復 |

正式 Bot 的電腦記錄顯示 CPU 曾被限流 946 次（累計 104 秒）。原因是 Docker `--cpus 2` 預設以 100 ms 為一期：Chromium 一忙就在一期開頭用完額度，整台電腦（包括畫面伺服器）要等到下一期，與行程優先度無關。現在改為 `--cpu-period 10000 --cpu-quota 20000`，上限不變。
