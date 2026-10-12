---
name: grok-build
description: 寫程式、建專案、修 bug、重構或審查程式碼時，交給 Grok Build CLI（grok）在 /workspace 直接思考並實作。
---

# Grok Build 寫程式

使用者要「寫程式／做網站／做工具／修 bug／重構／看 PR」時，不要自己一行行寫檔，交給電腦裡的 `grok`（xAI Grok Build）。你負責：整理需求、選資料夾、啟動、驗收、用 3 到 5 行回報。

## 1. 確認可用

```
terminal(command="command -v grok && test -f ~/.grok/auth.json && echo ready || echo login-needed")
```

- 沒有 `grok`：`npm install -g @xai-official/grok`。
- `login-needed` 且環境沒有 `XAI_API_KEY`：請使用者打開「電腦」面板，在終端機執行 `grok login` 用 SuperGrok／X Premium+ 登入。不要在對話索取金鑰或密碼。登入後再繼續。

## 2. 交辦

每個專案一個資料夾：`/workspace/projects/<簡短英文名>`，不存在就建立並 `git init`。把需求寫成一段完整的英文或中文 prompt：目標、技術限制、驗收方式（要跑哪些測試或指令）。

一次性工作（預設）：

```
terminal(command="grok --no-auto-update --always-approve --output-format json -p '<需求，含驗收方式；完成後執行測試並摘要改了什麼>'", workdir="/workspace/projects/<name>", timeout=600)
```

超過 10 分鐘的大工作改背景執行並追蹤：

```
terminal(command="grok --no-auto-update --always-approve -p '<需求>'", workdir="/workspace/projects/<name>", background=true, notify_on_complete=true)
```

接續同一個專案的修改用 `-c`（延續該資料夾最近的 session）：

```
terminal(command="grok --no-auto-update --always-approve -c -p '<補充修改>'", workdir="/workspace/projects/<name>", timeout=600)
```

只要審查、不准改檔時拿掉 `--always-approve`，並要求「只輸出 Markdown 結論」。

## 3. 驗收與回報

- 自己再跑一次關鍵指令（測試、建置或啟動），確認 grok 說的是真的。
- 失敗時把錯誤訊息原文用 `-c -p` 交回 grok 修，同一錯誤最多兩輪，之後向使用者說明卡在哪。
- 回覆只寫：完成了什麼、怎麼執行或打開、還沒做的部分，以及專案路徑。不要貼整段程式碼或 grok 的完整輸出。
