# Cua 遷移檢查（2026-09-07）

結論：`a.md` Phase 1 規格尚未全部勾完，但 **opt-in Cua 已可在現有 XFCE + Xvfb 桌面容器使用**。生產預設仍是 `legacy`。

本次在 Apple Silicon（linux/arm64）上以 `lazyboy/computer:local` + Cua Driver **0.23.2** 重跑隔離桌面驗收。

## 現況

| 規格 | 狀態 |
| --- | --- |
| 雙後端、Agent schema 不變 | 完成。`LAZYBOY_COMPUTER_DRIVER=legacy`（預設）或 `cua`。 |
| Docker 安裝 | 完成。Dockerfile 依 `TARGETARCH` 安裝 linux-arm64 / linux-x86_64，checksum 固定。 |
| `computer_observe` | 完成。截圖 + native AT-SPI 元素（`kind=a11y`、snapshot-scoped `cua:…` handle）+ 視窗列表。 |
| `computer_act` | 完成。pointer / type / key / scroll / focus / 單次 `drag`；native ref 走 Cua `click` / `set_value`。不支援的動作明確 `Unsupported`，不再偷偷回退 legacy。 |
| Browser | 完成。`existing_profile` attach、`semantic_v2` refs、navigate 限 http/https/about。 |
| 多螢幕隔離 | 完成。display `:1` 的 handle 送到 `:2` 會被拒絕；各自點擊只改自己的測試程式。 |
| pause / restart 後 Chromium cookie | 完成。adapter `--check-persistence` 在 `docker pause` 與 `docker restart` 後都通過。 |
| noVNC | smoke 確認 `:6080` 仍可連；完整 human takeover 端到端未另開測試。 |
| 示範錄製 | Cua `start_recording` + 既有 CDP recorder 仍在；未做真人 noVNC 示範驗收。 |
| Benchmark 文件 | 見 [cua-benchmark.md](cua-benchmark.md)。尚未對 legacy 做對照。 |
| 預設切到 Cua | **未做。** 完整 DoD（takeover、錄製、生產 metrics）未過前維持 legacy。 |

## 這輪修正

- Native 同一批 `computer_act` 共用一份觀察快照；先前每個動作都把 handle map 拿掉，導致 `wait` 後面的 ref 或連續兩個 ref 被當成過期。
- 拒絕的過期 ref 不再清掉該螢幕上其他仍有效的 handle。
- JSON `code != ok` 即使行程成功碼為 0 也當失敗（避免 drag 誤報成功）。
- CLI JSON 改走 stdin，避免輸入文字出現在 argv。
- 拖曳改打最小包含該點的視窗（避免點到覆蓋其上的 Chromium），focus 優先精確標題（避免 `LazyBoy Cua Smoke - Chromium` 搶走 GTK 視窗）。
- GTK 測資把 drawing area 座標轉成螢幕座標。
- Cua 選到但 binary 不在或 daemon 起不來會明確失敗。

## 驗證（本機 2026-09-07）

- `cargo test --locked -p lazyboy-control`：84 通過。
- `cargo check --locked -p lazyboy-api -p lazyboy-sandbox -p lazyboy-controld`：通過。
- `make cua-smoke`（`--repeat 10`）：
  - 原始 Driver smoke **10/10**（截圖、視窗、native click/type、Chromium attach、noVNC）
  - LazyBoy adapter **10/10**（GTK click / 中文 setvalue、過期 handle 拒絕、批次 native、drag、Chromium 表單）
  - 雙 display 隔離通過
  - pause/unpause 與 `docker restart` 後 cookie 仍在

## 如何啟用

預設不要改。要在本機明確跑 Cua：

```bash
make cua-smoke
# 或
docker compose -f docker-compose.yml -f docker-compose.cua.yml up -d --build
```

overlay 把 supervisor 的 `LAZYBOY_COMPUTER_DRIVER` 設成 `cua`，桌面映像標成 `lazyboy/computer:cua`，不會覆寫預設的 `lazyboy/computer:local` legacy 映像。改 flag 後必須重建桌面容器。
