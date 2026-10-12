# LazyBoy 改版：流程、Rust、介面與 Grok Build

日期：2026-10-11。這份是結論，細節數據見 `docs/benchmarks/`。

## 一句話結論

- **慢的是模型與工具，不是後端。** 改 Rust 幾乎不會變快，先不要重寫。
- **介面改成 Grok 風格**：工作中只顯示一行「思考中」，完成後過程收成一顆「已完成 · N 個步驟」，檔案只列重要的。（已完成）
- **寫程式交給 Grok Build**：Bot 遇到開發工作會用 `grok -p` 在 `/workspace/projects/<名稱>` 直接思考與實作，自己只負責驗收與摘要。（已完成）
- **名字改成 LazyBoy，換新 icon。**（已完成）

## 1. 後端要不要改 Rust？

不建議全面改寫。依 2026-10-07 的實測（三個真實任務，`product-latency-audit-2026-10-07.json`）：

| 花在哪裡 | 佔工作時間 |
| --- | --- |
| 等模型回應（168 次，中位數 5.6 秒） | **85%** |
| 執行工具（瀏覽器、抓網頁） | 12% |
| 後端準備請求 | 每次約 51 ms，< 1% |

就算後端快 10 倍，整件工作只會快不到 1%。另外，舊版 LazyBoy2 本來就是 Rust，換成 Python + Hermes 的原因是少維護一整套 Agent 核心；改回 Rust 會失去這個好處，而且 Hermes 本身是 Python，無論如何都要跨語言。

值得用 Rust 的只有兩處，等真的出現瓶頸再做：

1. **電腦串流代理（noVNC WebSocket）**：多 Bot 同時看畫面時，可以獨立成小型 Rust/Go 代理。
2. **檔案索引**：工作目錄超過數萬個檔案時。目前已改為略過 `node_modules` 等依賴資料夾，足夠應付。

## 2. 真正能讓它變快的流程優化

依效益排序：

1. **減少模型來回次數**（最大效益）。168 次請求裡，`browser_navigate` 失敗 10 次、`browser_wait` 4 次全失敗。已在 SOUL 加入「同一網址或操作失敗兩次就換方法」與「等待特定元素，不等固定秒數」。
2. **簡單問題用聊天模型**：「設定 → 模型 → 對話與工作分流」已支援；建議開啟分類器的正式分流（不只記錄），問答類可省 3–10 秒。
3. **開啟供應商的 prompt caching**：長任務每輪都重送整段上下文；有快取的供應商（Anthropic、DeepSeek、xAI）可降低首字延遲與費用。
4. **寫程式交給 Grok Build**：原本 Bot 一個檔案一個檔案寫要數十輪；交給 `grok -p` 一次完成，Bot 只看結果。
5. **電腦保持暖機**：容器冷啟動要等桌面就緒（上限 75 秒）；常用的 Bot 不要停電腦。

## 3. 顯示方式（Grok 風格）

研究 Grok、Cursor、Open WebUI、assistant-ui 的做法，共同點是：**過程預設收合成一行，答案是主角。**

| 之前 | 現在 |
| --- | --- |
| 工作卡片：標題、狀態、2 個步驟、過程說明、按鈕 | 一行「思考中 · 12 秒 · 3 個步驟 ›」＋目前在做的一步；看電腦／停止縮成小圖示 |
| 答案下方「執行了 N 個步驟 · M 則過程說明」 | 答案**上方**一顆「已思考 1 分 20 秒 · 6 個步驟 ›」，點開才看細節 |
| 產出檔案列 4 個 | 只列 3 個最重要的，其餘「+N」 |
| 回覆長度 | 預設「先給結論、不超過 8 行」，Bot 設定可改「極簡」 |

## 4. 檔案只顯示重要的

- 工作產出（成果卡）只計入重要檔案，最多 12 個，文件／圖片／報表排前面。
- 檔案面板預設只顯示重要檔案；建置輸出、快取、log、lock 檔收在「顯示全部」。
- `node_modules`、`__pycache__`、`venv`、`target` 等依賴資料夾不再列出（以前會吃掉 500 筆上限）。
- 判斷規則在 `server/files.py` 的 `important()`。

## 5. Grok Build 銜接

- Bot 映像安裝 `@xai-official/grok`（`box/Dockerfile`，`GROK_CLI_VERSION` 可固定版本）。
- 每個 Bot 第一次啟動時自動放入 `grok-build` 技能（`server/builtin_skills/`）；使用者刪除或修改後不會被覆蓋。
- 登入：在 Bot 的「電腦 → 終端機」執行一次 `grok login`（SuperGrok／X Premium+），憑證保存在該 Bot 的家目錄。供應商金鑰仍只留在產品服務端，不會傳進 Bot。
- 流程：釐清需求 → `grok --always-approve --output-format json -p '…'` → Bot 自己跑測試驗收 → 失敗把錯誤交回 `grok -c` 最多兩輪 → 回覆 3–5 行摘要與專案路徑。

## 6. 名稱與 icon

- 介面、頁籤、PWA、登入頁、預設 Bot 名稱都改成 **LazyBoy**。
- 為了不弄壞現有資料，內部名稱（`LAZYHERMES_*` 環境變數、`lazyhermes-bot-*` 容器、資料目錄）保留不變。專案資料夾 `LazyBoyHermes` 也沒改名，避免影響正在執行的服務。
- 新 icon：紫色漸層圓角方塊、閉眼打瞌睡的臉和「Zz」，32 px 也看得清楚。已更新 favicon、PWA 圖示與 iOS 啟動畫面。

## 7. 還沒做、建議接著做

1. 重建 Bot 映像讓 `grok` 生效：`docker build` 後在設定按「更新電腦」。
2. 專案根目錄可清掉的舊檔：`web/app.js`、`web/styles.css`、`web/icon.svg`、`web/sw.js`、`web/manifest.webmanifest`（舊版前端，現在都沒被引用）；`.run/` 有 594 MB 測試紀錄。
3. 用同一組蝦皮任務重測請求次數，確認第 2 節第 1 點的效果。

參考：[Hermes Grok 技能文件](https://hermes-agent.nousresearch.com/docs/user-guide/skills/optional/autonomous-ai-agents/autonomous-ai-agents-grok)、[Grok Build 介紹](https://en.wikipedia.org/wiki/Grok_Build)、[assistant-ui Chain of Thought](https://www.assistant-ui.com/docs/guides/chain-of-thought)、[Open WebUI 推理模型顯示](https://docs.openwebui.com/features/chat-conversations/chat-features/reasoning-models.md)。
