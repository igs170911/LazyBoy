# hermes-agent 對照：harness 與 Cua 合作的順暢度（2026-09-08，更新：2026-09-09）

研究對象：[`NousResearch/hermes-agent`](https://github.com/NousResearch/hermes-agent)，commit `abd83ab560327c58f17f8e2ecd9be0307d5c9cf9`（12,269 檔）。本輪擷取 `tools/computer_use/` 全 14 檔與官網 computer-use 使用者手冊；下文 hermes 路徑皆為該 repo 相對路徑，LazyBoy 路徑附 `file:line`。

要回答的問題不是「要不要換一套 Cua 用法」，而是：**hermes-agent 在同樣的 cua-driver 限制下做了哪些選擇，是我們的 harness 可以拿來讓模型用得更順的？**

## 結論

**可以改善，而且不需要改動 Cua 串接層。** hermes-agent 的 `computer_use` 同樣落到 cua-driver（差別是它走 MCP over stdio，我們走 per-call CLI），兩邊面對的是同一個 Linux AT-SPI/X11 桌面、同一批 driver 工具、同一種「動作可能送達但無法證明生效」的不確定性。差距幾乎全在 **harness 這一層**：

1. hermes 把 Cua 回給我們的**語義結果（effect / escalation / verified）誠實地換算成一個 verdict 交給模型**，並在 prompt 裡明令「已確認生效不得重打輸入」。我們讀了這些欄位，只用來決定要不要自己重試一次，**模型本身看不到**。
2. 我們用 **byte-exact 的 `frame_id`** 判斷「畫面沒變」，真實桌面上時鐘與游標一直動，等於這個判斷永遠不成立——重複點同一個按鈕的教練機制因此形同虛設。repo 裡已有感知級簽名，但只用在教學影格。
3. 我們的 observation 文字**同時**列出元素清單與完整 `elements` JSON，兩份內容、皆無上限，dense 桌面一輪就能燒掉數千 token。hermes 對元素數量與標題長度都有預算，多出來的 spill 到檔、讓模型自行 `read_file`。

其餘是次要項（串行觀察呼叫、非視覺模型、driver 版本策略、safety 硬擋）。以下逐項附證據。

## 比較基準

| | LazyBoy | hermes-agent |
| --- | --- | --- |
| 呼叫方式 | 每次動作 spawn 一個 `cua-driver call` CLI 行程（`crates/control/src/cua/client.rs:273`） | 常駐 MCP session over stdio，CLI 僅當 fallback（`tools/computer_use/cua_backend_session.py`） |
| 驅動版本 | 硬 pin `0.23.x`，不符即 unhealthy（`crates/control/src/cua/mod.rs:39`） | 不 pin，改成 runtime contract gate（version floor + 必要 argv 集合）+ 逐工具 capability（`cua_backend_driver.py:154`） |
| 工具數 | 21 個獨立 tool（`crates/api/src/tools.rs:64`） | 單一 tool + `action` 判別參數，省每輪 schema token（`tools/computer_use/schema.py`） |
| 結果語義 | 語義結果換算成 `ActionVerdict`，並寫進 tool result 第一行（`crates/control/src/cua/client.rs:442`） | `verified`/`effect`/`escalation`/`degraded` 換算成 `verdict.decision`（`tool.py:409`） |
| 失敗後重放 | 工具層 150s 與底層 120s 都 fail closed：不重放、訊息講明效果不明（`crates/api/src/runs.rs:1288`、`crates/control/src/cua/client.rs:199`），並把 session 標 suspect、下次呼叫前重建（`crates/control/src/cua/client.rs:247`） | 傳輸/逾時一律 fail closed、**絕不重放 mutation**，並將 session 標記 suspect、下次呼叫前重建（`cua_backend_session.py:71`, `:181`, `:495`） |

## 逐項對照

每一節講的都是**診斷當下**的狀態；已落地的章節在標題標明，位置與對應測試統一列在〈已交付〉。

### 1. 逐動作 verdict 沒有回到模型（最高價值，已實作）

hermes 的做法（`tool.py:409 _classify_action_result`、`tool.py:429 _action_payload`、`cua_backend_parse.py:_action_result_from`）：把 driver 的 `verified`、`effect ∈ {confirmed, unverifiable, suspected_noop}`、`escalation.recommended`、`path`、`degraded`、`code` 原帶回傳，再換算成一個模型看得懂的決定：

- `confirmed` → `{"decision": "done"}`
- `unverifiable` → `{"decision": "verify_fresh_state"}`，附「先重擷再決定，**不可以只因為有 escalation 建議就重打輸入**」
- `suspected_noop` / 有 `code` → `{"decision": "escalate"}` + 建議模式
- 傳輸成功但無語義證據 → 同樣是 `verify_fresh_state`（原文註解：*transport success without semantic proof is not proof of effect*）

LazyBoy 現況：`recommended_delivery()` 讀 `/escalation/recommended`（`crates/control/src/cua/client.rs:430`）後自己換 `delivery_mode` 重試一次（`crates/control/src/cua/client.rs:226`）；`effect` 只在 `refused|error` 時當失敗（`crates/control/src/cua/client.rs:484`）。往上一層就斷了：`ActionResult { clipboard_text, completed, observation }` 原本**沒有逐動作訊號欄位**，模型只會看到「completed N computer action(s)」加下一張截圖。**（已修：`ActionResult.verdict`，`crates/control/src/sandbox.rs:190`，見〈已交付 A〉。）**

後果：一個 `unverifiable` 的輸入（Linux AT-SPI 很常見）在我們這裡跟 `confirmed` 完全一樣。模型無法判斷該重看還是重打，只能從下一張截圖猜——這就是重複點擊的起點。

### 2. 「畫面沒變」是 sha256 byte-exact（最高價值，已修正）

`frame_id = sha256(PNG)`（`crates/control/src/observe.rs:16`），比較用 `frames_match`（`crates/control/src/observe.rs:133`）。桌面只要有時鐘、游標閃爍、notification、動畫，sha256 就不同 → `unchanged` 永遠是 false → `miss_streak` 永不累積（`crates/api/src/tools.rs:1059`）。兩個教練因此失靈：

- `note_click_result`（`crates/api/src/tools.rs:1055`）的「別再重複同一個點擊」建議
- `should_block_stale_click`（`crates/control/src/actions.rs:139`）的硬性擋掉（streak ≥ 2 且同一目標）

失靈後模型會一直重複同一個無效點擊，直到 LoopGuard 的輪次政策（`crates/harness/src/policy.rs`）把整個 run 停下——花掉整輪預算才換來一句本來可以在第一輪就講的話。

repo 裡**已經有**對的工具：`frame_signature` / `signatures_similar`（`crates/control/src/observe.rs:143`, `:156`，32×18 灰階、變動 < 2% 視為相似），但目前只用在教學關鍵影格（`crates/api/src/skills.rs:858`）。

### 3. Observation payload 沒有預算，而且重複兩份（已修正重複與上限）

`observation_text`（`crates/api/src/tools.rs:471`）先印 `format_ui_elements`（`crates/control/src/observe.rs:69`，`[id] title` 串接、無上限），**接著**又 dump 整個 `elements` JSON（含 `selector`、`role`、幾何）。上層 `merge_ui_elements`（`crates/control/src/a11y.rs:6`）也不設上限。Windows 標題允許 256 字（`crates/control/src/cua/mod.rs:542`），Chromium 的 a11y 樹還會把整段訊息內文當成 label。只有 checkpoint 寫入時才會被 `shrink_checkpoint` 截到 48KB（`crates/api/src/runs.rs:1933`）——那是存檔保護，不是送給模型的預算。

hermes 的預算（`tool.py:441`）：`_DEFAULT_MAX_ELEMENTS = 100`、`_MAX_ELEMENT_LABEL_CHARS = 120`、summary 只列 40 行，被截到的**完整樹 spill 到檔**並告知模型可用 `read_file`/`search_files` 取回；原因是「Discord/Slack via UIA 把整個訊息內文當 label，沒上限會爆掉 tool-result 預算並外洩聊天文字」。它甚至刻意讓 **multimodal 回應不附 `elements` array**（已有截圖時兩份是浪費，`tool.py:549 _capture_response`）。

### 4. 未知結果沒有徹底 fail closed（已實作）

hermes 把「效果不明確」當成獨立類別（`cua_backend_session.py:71 _UNKNOWN_OUTCOME_MESSAGES`）：傳輸失敗或 MCP 逾時都回 `isError` + `next_step: "fresh_state"`，訊息裡明講「動作**可能已經生效**，所以 Hermes 沒有重放它」。`_TRANSPORT_REPLAY_SAFE_TOOLS`（`:181`）是一組**唯讀**工具白名單，只有它們允許在傳輸錯誤後重試（`:495`, `:504`）；mutation 一律不重放。

LazyBoy：工具層 150 秒逾時的措辭已經對了（`crates/api/src/runs.rs:1288`：*Its effects are unknown … never repeat a step that already worked*）。但底層 CLI 的 120 秒逾時只拋 `ControlError::Timeout`（`crates/control/src/cua/client.rs:381`），訊息是 `computer action timed out`——沒有任何指示，模型的自然反應就是重打一次。同一個不確定性有兩種處理標準。**（已修：`crates/control/src/controller.rs:59` 的 `Timeout` 訊息現在明講效果不明、先觀察、不得重放成功的步驟。）**

### 5. 逾時後不重建 driver session（已實作）

hermes 一次 timeout 就把 session 標成 suspect，下次 computer-use 呼叫前先重建。LazyBoy 只在 driver 回報 `session_ended` 時 revive（重發 `start_session`，`crates/control/src/cua/client.rs:151-163`）。逾時／半死 socket 不會進那個分支，於是同一個可疑 session 會繼續被用下去，後續每個動作的結果都不可信。**（已修：逾時即標記 suspect，下次 mutation 前重建 session，`crates/control/src/cua/client.rs:247`。）**

### 6. 一次觀察要 spawn 四次 CLI（已併行）

`observe_display` 原本串行做：`get_desktop_state`（寫檔讀 PNG）→ `screen_size`（必要時）→ `cursor` → `windows` → `native::observe`，每次都是一個行程 + socket 往返，全部加在模型等待時間裡。**（已修：`crates/control/src/cua/mod.rs:487`，cursor 與 windows 用 `tokio::join!` 併行（`crates/control/src/cua/mod.rs:512`）。cursor 不省——`scroll_point` 要靠它瞄準，而且併行之後已不額外花時間。）**

hermes 另外示範了「用本機命令取代一次 driver 呼叫」：`_select_capture_target`（`cua_backend_capture.py:58`）用 `xprop _NET_ACTIVE_WINDOW`（2 秒 timeout）判作用中視窗，只在 X11 `z_index` 同值時才採用，避免多一次 driver 往返。

### 7. 空結果會跨 transport 重取（已區分「失敗」與「空」）

hermes `_fetch_or_refetch`（`cua_backend_capture.py:150`）：MCP 成功但內容為空時，改用 CLI 再取一次——因為「成功但空」在 Linux 上常常是 transport 問題，不是桌面真的沒東西。我們原本也沒有防禦：`windows(...).unwrap_or_default()` 把失敗直接變成「沒有視窗」，模型看到的是空桌面。**（已修：失敗與「真的沒有視窗」分開，觀察會標 partial，見〈已交付 E〉；跨 transport 重取本身不做，理由見〈不做〉。）**

### 8. 非視覺模型完全不能用電腦（已給 ax-only 觀察）

hermes 支援 `capture(mode='ax')`（不附截圖、只有元素樹）與 `vision_routing.py`：不確定主模型能否吃圖時 **fail closed 到 aux vision** 先把截圖轉文字。LazyBoy 的 `vision_guard`（`crates/api/src/tools.rs:455`）原本對非視覺模型直接回「請改選視覺模型」，連純元素樹的觀察都一起擋掉。**（已修：ax-only 降級路徑已做，非視覺模型保留元素樹觀察、擋掉像素工作，見〈已交付 H〉；aux vision 需要第二模型的呼叫管線，延後。）**

### 9. 驅動相容性：硬 pin vs contract gate（已加 capability probe）

hermes 明確拒絕 version pin（`cua_backend_driver.py:19` 註解：上游安裝器一律抓最新版，pin 只是幻象），改檢查「version floor + 必要 argv 集合 + 逐工具 capability tokens」，舊 driver 不支援 foreground 就**明確 refused 而不是降級假成功**。LazyBoy 只認 `0.23.x`（`crates/control/src/cua/mod.rs:39`，且有測試驗證 `0.24.0` 視為不符，`crates/control/src/cua/mod.rs:951`）。

取捨要講清楚：**在生產環境我們現在這樣比較好**（可重現、驗收過就是驗收過）。hermes 的教訓是「pin 要配 capability probe」——當我們升到 0.24 時，不該只改常數，而該在 unhealthy 訊息裡點名**缺哪個 argv**，否則現場只會看到「driver unhealthy」這種查不出原因的話。**（已做：`cua-driver manifest` + `list-tools` 的 capability probe，見〈已交付 G〉；pin 本身保留。）**

### 10. Safety 與 schema 細節（已實作硬擋與建議）

hermes 有幾件我們沒有的硬擋（`tool.py:47`、`:51`、`:57`、`:68`）：

- 危險 `type` 內容模式擋掉（`curl … | bash`、`sudo rm -rf`、fork bomb）
- 危險按鍵組合硬擋，且 `_canon_key_combo` 會把 `ctrl-alt-delete` 這種底線寫法正規化後才比對
- `_input_target_mismatch`：輸入落到別的視窗時不報 ok（防「打錯視窗還說成功」）
- 未知 action 回「did you mean X?」，而不是丟一個 schema 錯誤讓模型亂猜
- tool schema **byte-frozen** 以保 prompt cache 命中
- `capture_after=true`：動作回應直接附新截圖，省一次 round-trip

LazyBoy 已有同級品的部分：SOM 數字疊字 `overlay_elements`（`crates/control/src/overlay.rs:47`）、容許 `#12`/`[7]` 等寫法的 `element_id`（`crates/control/src/actions.rs:49`）、歷史裡只留最近一張截圖（`crates/api/src/runs.rs:1905`）、批次動作 `MAX_COMPUTER_ACTIONS`。

### 11. LazyBoy 已經比較強的地方（不用移植）

輪次政策與停看聽（`crates/harness/src/policy.rs`）、takeover/resume、checkpoint 續跑、記憶 recall 預算、steering、`make cua-smoke` 端到端驗收。hermes 沒有多使用者、容器隔離、真人接管這層——它的問題意識是單一使用者長期佔用一台機器。

## 改造清單

### P0（順暢度差異最直接，改動侷限在 harness）

| 項目 | 改動點 | 預期效益 | 風險 |
| --- | --- | --- | --- |
| verdict 上層給模型（已做） | `ActionResult` 增 `verdict`/`effect`/`escalation`（`crates/control/src/sandbox.rs:146`）→ `crates/sandbox/src/docker.rs` 帶過 controld wire → `observation_text` 一行結論 | 模型知道「已送達未證明」該重看、不該重打；少一整個重複點擊循環 | 動到 controld wire format（向後相容：全部 `#[serde(default)]`）；措辭要短，否則反而變長 |
| 感知級 unchanged 用於教練（已做） | `ToolCtx` 加簽名欄位，`pack_observation` 算 `frame_signature`；截圖是否附行的 byte-exact 邏輯**不動** | 同一目標的重複點擊在累計兩次「無可見變化」後被擋掉；不再靠 LoopGuard 收場 | 簽名比對是啟發式；只影響教練建議、不影響送不送圖，風險可控 |

### P1

- **observation payload 預算（已完成）**：元素逐行（`[id] role "標題" @ x,y`）+ 數量上限 + `+N more not listed` 提示，**移除重複的 `elements` JSON**（模型只需要 id，見 `element_id`）；保留 frameId/尺寸/activeWindow 小 metadata。代價：一旦截斷，模型無法直接看到被省略的元素——先靠提示叫它捲動重看，之後再考慮 hermes 式的「寫到檔 + `read_file`」。
- **底層 timeout 改成 fail closed 措辭（已完成）**：`ControlError::Timeout` 的訊息加上「效果不明、可能已生效、先觀察再決定、不得重放」，並與 `runs.rs:1288` 的措辭統一。
- **逾時後標記 suspect 並重建 session（已完成）**：在 `CuaClient` 記一次逾時，下次 mutation 前先 `start_session`；失敗就明確報 driver unavailable。
- **空結果不當成空桌面（已完成）**：`list_windows` 失敗要與「真的沒有視窗」區分（現在是 `unwrap_or_default()`）。

### P2

- **（已完成）** `observe_display` 的 cursor 與 window 清單改成 `tokio::join!` 併行（`crates/control/src/cua/mod.rs:512`）。cursor **不省**：`scroll_point` 靠游標位置瞄準，併行之後它已經不額外花時間。
- **（ax-only 已完成，aux vision 延後）** 非視覺模型保留元素樹觀察，只有像素工作仍擋（〈已交付 H〉）。
- **（已完成）** driver 不相容時點名缺少哪個 argv／哪個工具，而不是只說 unhealthy（〈已交付 G〉）。
- **（已完成，`capture_after` 除外）** 危險 `type` 內容與按鍵組合硬擋、未知 action 給建議（〈已交付 F〉）；`capture_after` 評估後不做（〈不做〉）。
- **（改做守門測試）** 21 個 tool 的 schema 合併會弄壞 checkpoint 裡舊的 `tool_calls`，改以序列化穩定性測試保住快取鍵（〈已交付 I〉、〈不做〉）。

## 已交付

清單來自本節的診斷，P0/P1/P2 全部落地或在〈不做〉裡記錄理由。每項給位置與測試名稱，測試都可在 repo 內直接搜尋。

### A. verdict 回到模型（P0）

- driver 回包的 `effect` / `verified` / `escalation` / `degraded` / `code` 換算成 `ActionDecision{Done, VerifyFreshState, Escalate}`（`crates/control/src/sandbox.rs:151`）與 `ActionVerdict`（`crates/control/src/sandbox.rs:171`），掛在 `ActionResult.verdict`（`crates/control/src/sandbox.rs:190`）。
- 換算規則在 `crates/control/src/cua/client.rs:442 action_verdict`：`confirmed` 壓過任何 escalation 建議、`suspected_noop` 或拒答 code → `escalate`、其餘證據 → `verify_fresh_state`，**完全沒有語義證據就回 `None`**（transport 成功不等於生效）。測試：`a_confirmed_effect_outranks_an_advisory_escalation`、`unverifiable_effect_becomes_verify_fresh_state`、`suspected_noop_and_refusal_codes_escalate`、`a_reply_without_semantic_evidence_claims_nothing`。
- 批次動作取最嚴重的一筆（`crates/control/src/cua/mod.rs:701 merge_verdict`，`ActionDecision` 的列舉順序就是嚴重度），經 controld wire（`crates/controld/src/main.rs:166`）回到 `crates/sandbox/src/docker.rs:263`；兩側都是 `Option` + `#[serde(default)]`，**舊 controld 只是不带 verdict，不會解析失敗**。
- 模型看到的是 tool result 的第一行：`crates/api/src/tools.rs:1077 verdict_note` 加 `crates/api/src/tools.rs:1096 with_verdict`（前置，不會被截圖說明蓋掉）。已確認 → 「不得重複這個動作」；未證明 → 「先讀新截圖，絕不重打可能已生效的輸入」；無效果 → 「換做法，別重打同一輸入」。測試：`crates/api/src/tools.rs:1983 verdict_tests`。
- 「已確認生效」同時算進 miss streak 的判斷（`crates/api/src/tools.rs:902`），免得面板時鐘把一次成功的點擊當成「畫面沒變」。
- **代價**：verdict 要 desktop image 裡的 controld 重建後才會出現；本輪**沒有重啟 live 容器**，所以線上暫時仍是「沒有 verdict」而非錯誤答案。

### B. 感知級 unchanged 只供教練判斷（P0）

- `ScreenChange{Identical, Similar, Changed}` 與 `screen_change_between`（`crates/control/src/observe.rs:174`、`crates/control/src/observe.rs:184`），`ToolCtx.previous_signature` 每輪更新；觀察文字分三种標記（byte-exact `(screen unchanged)`、感知 `(no visible change)`、無標記）。
- 附不附截圖**仍然只看 byte-exact**，細微視覺變化不會被藏起來；miss streak 與 `should_block_stale_click` 讀感知狀態，所以時鐘不再把 streak 歸零。測試：`screen_change_calls_a_clock_tick_similar_not_identical`。

### C. 觀察清單有預算（P1）

- `format_ui_element_lines`（`crates/control/src/observe.rs:89`）取代「清單 + 完整 `elements` JSON」兩份內容：每個控制一行 `[id] role "標題" @ x,y`，標題 60 字、元素 120 個（`crates/api/src/tools.rs:467 MAX_LISTED_ELEMENTS`），超出以 `+N more not listed` 說明；frameId／尺寸／activeWindow 小 metadata 保留。測試：`lists_each_control_once_and_names_what_was_dropped`、`element_lines_cap_the_count_and_long_labels`。

### D. 未知結果徹底 fail closed（P1）

- `ControlError::Timeout` 的訊息（`crates/control/src/controller.rs:59`）改成「效果不明、可能已生效、先觀察再決定、不得重放已成功的步驟」，與 `crates/api/src/runs.rs` 工具層 150 秒的措辭一致；保留 `timed out` 子串，`monitor.rs`／`runs.rs`／`mcp.rs` 的失敗分類不受影響。
- `CuaClient` 記 suspect 顯示（`crates/control/src/cua/client.rs:23`）：mutation 逾時即標記該 display **且永不重放**；下一次 mutation 前先 `repair_suspect_session`（`crates/control/src/cua/client.rs:247`）重建命名 session，成功的話補一次游標動態設定，再逾時就保留標記繼續 fail closed。唯讀工具（`read_after_session_restart`）不付這多出來的 round-trip。

### E. 空視窗清單不等於空桌面（P1）

- `list_windows` 失敗不再 `unwrap_or_default()` 變成「沒視窗」：失敗時把 `native_observation_complete` 關掉（`crates/control/src/cua/mod.rs:564`），欄位語義寫進 `crates/contracts/src/action.rs:145`。
- 觀察文字在清單標題後加 coverage（`crates/api/src/tools.rs:483`）：`(partial: some windows did not report controls, so a missing entry is not proof)`——模型正是在這一行決定「按鈕不存在」。測試：`an_incomplete_sweep_says_a_missing_control_is_not_proof`。

### F. Safety 硬擋與「你是不是打錯」（P2）

- 未知 action 名稱給出最近的正確名稱，或指向正確工具（`crates/control/src/actions.rs:146 KIND_ALIASES`、`:183 OTHER_TOOL_HINTS`、`:190 nearest_action_kind`）：`screenshot`／`capture`／`observe` → `computer_observe`，`snapshot` → `browser`。測試：`an_unknown_action_name_points_at_the_right_thing`。
- 按鍵組合先正規化再比對（`:234 BLOCKED_KEY_COMBOS`、`:242 canonical_keys`、`:260 blocked_key_combo`），`ctrl+alt+delete`、`ctrl-alt-delete`、`super+l` 一律擋（`session_killing_shortcuts_are_blocked_however_they_are_spelled`）。
- 危險 `type` 內容擋掉（`:277 blocked_text`）：`curl … | sh`、`rm -rf /`、fork bomb、`of=/dev/`，比對前做空白折疊，所以多空格／換行繞不過（`destructive_typed_text_is_blocked_and_the_work_is_not`）。

### G. driver 不相容時點名缺什麼（P2）

- pin 保留（理由見〈不值得照搬〉），但失敗時不再只說 unhealthy：`cua-driver manifest`（`crates/control/src/cua/client.rs:105`）比對我們真的會 spawn 的 argv（`crates/control/src/cua/mod.rs:52 REQUIRED_CLI_ARGS`：`call --socket`、`call --screenshot-out-file`、`status --socket`），`cua-driver list-tools`（`crates/control/src/cua/client.rs:123`）比對我們真的會 call 的 25 個工具（`crates/control/src/cua/mod.rs:59 REQUIRED_TOOLS`，與 0.23.2 逐一对過）。
- 輸出是「driver CLI is missing: call --screenshot-out-file」或「driver tools are missing: X」，全部都在時則是「…only the version series differs」（`crates/control/src/cua/mod.rs:136 capability_gap`）。兩個探測都唯讀、10 秒上限、**只在 health 失敗時跑**，老 driver 沒這兩個 verb 就靜默回到舊訊息。測試：`an_older_driver_is_named_by_the_verb_flag_it_lacks`、`the_pinned_driver_surface_passes_the_cli_contract`、`a_missing_tool_is_named_instead_of_a_bare_unhealthy`、`the_tool_list_keeps_names_and_drops_banner_noise`。

### H. 非視覺模型可以讀桌面（P2）

- 拆成 `gui_blocked`（`crates/api/src/tools.rs:440`，真人接管時全擋）與 `vision_guard`（`crates/api/src/tools.rs:455`，只有**像素工作**需要視覺模型：`computer_act`、`browser`、`shell`、`open_path`、`launch_app`、`connection_check`、`use_saved_login`）。
- `computer_observe` 與 `wait` 改用 `gui_blocked`：非視覺模型照樣拿到元素樹文字，`pack_observation` 不附圖並加註 `crates/api/src/tools.rs:1112`「(elements only: this model cannot see the screen)」，順帶省掉 SOM overlay 與圖片 token。
- 系統提示跟一句改寫（`crates/api/src/runs.rs:50`）：點擊／輸入／瀏覽需要視覺模型，純文字模型仍可用 `computer_observe` 讀元素樹。

### I. prompt cache 的守門測試（P2）

- `tool_definitions`（`crates/api/src/tools.rs:64`）的位元組就是每輪請求的快取鍵。三條測試：`the_tool_schema_is_byte_stable_across_calls`、`tool_names_are_unique_and_open_with_the_computer_pair`、`memory_tools_are_appended_so_the_desktop_schema_never_moves`（記憶工具只能附加在尾端，桌面那半不能因為開關而位移）。位置：`crates/api/src/tools.rs:2037 tool_schema_tests`。

## 不值得照搬

- Python 重構、hermes 自己的 MCP session/embedded daemon 管理：我們的 per-call CLI 在容器模型下可重現性更高，換 transport 是另一個案子。
- macOS/Windows 專屬路徑（`windows_hide_flags`、Quartz、UIA 特例）與 desktop app 整合：LazyBoy 只做容器內 Linux 桌面。
- 取消 driver pin：與我們的驗收政策相反（見第 9 節取捨）。
- hermes 的 tool-result spill 到 host 檔：我們的沙箱檔案系統與 workspace 語義不同，先要用再設計。

## 不做（清單裡剩下的，連理由）

- **`capture_after`**：`ActionRequest.observe`（預設 true）已經讓動作回應直接附上新觀察，再疊一個 driver 層的 `capture_after` 是把同一件事做兩次，只會多出一個會不一致的開關。
- **21 個 tool 合成單一 `action` 判別**：省的是每輪幾 KB 的 schema token，代價是 checkpoint 裡舊的 `tool_calls` 名稱失效——續跑的 run 會開始報 unknown tool。改用 〈已交付 I〉 的序列化穩定測試保住現有的快取鍵。
- **aux vision（先用輔助視覺模型把截圖轉文字）**：repo 內沒有第二模型的呼叫管線（provider 選擇、額度、錯誤處理都要一條線），本輪先把「非視覺模型完全不能用」降級成「能讀不能動」，剩下的記在這裡。
- **跨 transport 重取（hermes `_fetch_or_refetch`）**：我們只有一條 per-call CLI transport，沒有第二條可以換過去重取；同樣的懷疑改用 〈已交付 E〉 的「不完整」標記表達。
- **取消 driver pin**：保留 `PINNED_DRIVER = 0.23`（`crates/control/src/cua/mod.rs:39`，相容性測試在 `mod.rs` 的 `a_new_minor_series_is_not_compatible`）。hermes 的教訓不是「不該 pin」，而是「pin 要配 capability probe」——現在升 0.24 時，unhealthy 訊息會點名缺哪個 argv／工具，而不是只說不相容。

## 驗證

- `cargo test --workspace --locked --no-fail-fast`：contracts 4、control **100**、harness 36、sandbox 2、supervisor 2 全綠；api **89 通過、2 失敗**——`memory::tests::database_enforces_agent_scope_and_queries_do_not_leak` 與 `monitor::tests::memory_usage_is_historical_scoped_and_respects_deletion` 需要 `make postgres` 的測試資料庫（`PoolTimedOut`），**改動前即失敗**，與本輪無關。
- 新增測試：control 端 verdict 4 條、safety 3 條、capability probe 4 條；api 端 `verdict_tests` 3 條、觀察清單/coverage 3 條、`tool_schema_tests` 3 條（名單見〈已交付〉）。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo fmt --all --check`：乾淨。
- capability probe 的比對基準用 `image/computer` 那份 image 離線跑 `cua-driver manifest` 與 `list-tools` 取得（0.23.2），`REQUIRED_TOOLS` 25 項全部有廣告；探測本身不在 live 容器上驗證。
- 端到端順暢度仍須 `make cua-smoke`／`make computer`。**本輪未重啟、未重建 live 容器**（`lazyboy-api-1`、`lazyboy-supervisor-1`、`lazyboy-postgres-1`、paused 的 `lb-team-local-space`），所以 verdict 上層與 wire 變更要在下一次鏡像重建＋驗收桌面重跑後，才會在線上看見；重複點擊的實際改善同樣要那一次才算數。
