# 開發指南

[← 回到 README](../README.zh-TW.md)

### 本機開發

```bash
make dev              # 準備 .env、PostgreSQL、電腦映像
make dev-supervisor   # 終端 1：Supervisor :7091
make dev-api          # 終端 2：API :3101
```

前端熱更新：

```bash
cd apps/web
npm install
npm run dev           # http://127.0.0.1:5173
```

### 檢查與測試

```bash
# 有資料庫的 Rust 測試需要 127.0.0.1:5434，先用 dev overlay 啟動 Postgres
make postgres

make lint               # clippy（-D warnings），政策見下方
make test               # Rust 測試（需要 127.0.0.1:5434）
make audit              # 依賴漏洞、授權、來源掃描（需要 cargo-deny）

cd apps/web
npm run typecheck
npm run build

cd ../..
node --test tests/frontend.test.mjs
python3 tests/log-rotation.test.py

# Cua Driver 能否控制現有 XFCE + Xvfb 桌面（會建 computer image）
make cua-smoke
# 結果摘要見 docs/cua-compatibility.md、docs/cua-review.md
# 生產路徑只使用 Cua；更新後請重建 supervisor 與桌面映像。

# Python 整合測試用 docker compose exec 連進 Postgres，自己建一次性資料庫後清掉
python3 tests/retention.test.py
python3 tests/run-resume.test.py
python3 tests/run-activity.test.py
```

`make postgres` 才會發布 `127.0.0.1:5434`（`docker-compose.dev.yml`）；整套堆疊已經用
基礎設定跑著時，請改用 `export COMPOSE_FILE=docker-compose.yml:docker-compose.dev.yml`
再 `docker compose up -d`，否則後續的 `docker compose` 指令會拿基礎設定重建 Postgres、
把發布埠拿掉。

### Rust 品質檢查

工具鏈以穩定版為準：workspace 宣告 `rust-version = "1.98"`（MSRV，代表 let-chains
等語法下限），容器用 `rust:1-*` 映像會自動跟最新小版。本機更新只需：

```bash
rustup update stable && rustc --version
```

Lint 政策集中在三處，新增 crate 時只要補 `[lints] workspace = true` 就會繼承：

- 根 `Cargo.toml` 的 `[workspace.lints]`：不安全程式碼預設警告、未使用的
  `Result`/`Future` 直接拒絕，並用 `unused_crate_dependencies` 抓多餘依賴
  （不需要另外裝 `cargo-machete`）。
- `clippy.toml`：閾值類設定。`too-many-arguments-threshold = 10` 是因為 handler
  與 run/vault/voice 協助函式本來就要帶 state + actor + 多個 id，超過 10 個參數才會警告。
  `cargo-clippy` 只看得到啟動目錄下的 `clippy.toml`，請一律在 repo 根目錄跑 `make lint`。
- 各 crate 的 `[lints] workspace = true`。

要放寬一條 lint 時，請在**呼叫點**加 `#[allow(...)]` 並附一句理由（例：
`crates/api/src/vault.rs` 測試裡的 `#[allow(unsafe_code)]`），不要在工作區層級關掉規則。

`make fmt` 會重排整個 workspace；目前倉庫仍有歷史格式偏差，`make fmt-check` 會列出來，
等到一次性重整後再併入 CI。CI 目前只需 `make lint` + `make test`。

供應鏈檢查用 `cargo-deny`（`make audit`，設定見 `deny.toml`）：RustSec 漏洞、授權白名單、
依賴來源。它不是內建工具，第一次要先 `cargo install --locked cargo-deny`。
已知無法升級的項目會寫進 `deny.toml` 的 `ignore` 並附原因與重檢時機。

### 映像與 CPU 架構

三個映像（`image/computer`、`image/api`、`image/supervisor`）一律用 `scripts/build-image.sh`
建置。支援的架構是 **linux/amd64 與 linux/arm64**，這個上限由上游二進位發行決定，不是實作取捨：

- Cua Driver `cua-driver-rs-v0.23.2` 的 Linux 發行檔只有 `linux-x86_64` 與 `linux-arm64`。
- ONNX Runtime `v1.24.1` 的 Linux 發行檔只有 `linux-x64` 與 `linux-aarch64`。

`image/computer/Dockerfile` 依 `TARGETARCH` 選檔並驗證 SHA256，`scripts/fetch-onnxruntime.sh`
同樣只認這兩種架構，其他架構直接建置失敗；`build-image.sh` 在註冊 QEMU 與開始跨修編
之前就先擋掉不支援的 `--platforms`，不會燒掉數 GB 才報錯。

```bash
make computer            # 本機架構，結果直接進本地 image 庫
make computer-multi      # amd64 + arm64 -> dist/lazyboy-computer-multi.oci.tar
make images-multi        # 三個映像都跨修 amd64 + arm64
make images-multi PUSH=1 # 直接發布 manifest list 到 registry
```

多架構需要 `docker-container` builder 與 QEMU binfmt，`build-image.sh` 會自行建立 `lazyboy`
builder 並註冊 binfmt；主機本身的架構會跳過註冊，否則在特權容器內會誤報失敗。
不推送 registry 時輸出 OCI archive（`nerdctl load -i <檔>` 可載入），因為 Docker 的
`docker` driver 無法匯出 manifest list。

### 專案結構

```text
LazyBoy/
├── apps/web/                 React 前端與 noVNC 頁面
├── crates/
│   ├── api/                  對外 API 與 Agent 執行迴圈
│   ├── contracts/            跨元件資料契約
│   ├── control/              瀏覽器與桌面控制
│   ├── controld/             容器內控制服務
│   ├── harness/              模型與語音後端
│   ├── sandbox/              API 到 Supervisor 的抽象
│   └── supervisor/           Docker 生命週期管理
├── image/                    API、Supervisor、電腦映像
├── migrations/               SQLx PostgreSQL migrations
├── tests/                    Rust 以外的契約與回歸測試
├── scripts/                  環境初始化與執行工具
├── docs/hero.html            README 首頁視覺原稿
├── docs/workflow.html        可互動產品流程圖
├── docker-compose.yml        正式堆疊
├── clippy.toml               Clippy 閾值（要在 repo 根目錄執行才讀得到）
├── deny.toml                 cargo-deny 供應鏈政策（make audit）
└── Makefile                  常用開發與部署指令
```

## 公開發布準備

對外發布前，可依下列清單補齊授權、貢獻流程與自動化檢查。只有在對應結果已公開時才加入品質徽章。

若要取得可公開查驗的安全與品質徽章，建議依序完成：

- [ ] 在公開 GitHub repository 建立正式 mirror；OpenSSF 的公開查驗以 GitHub 為主要整合目標。
- [x] Apache 2.0 `LICENSE`
- [ ] 補 `SECURITY.md`、`CONTRIBUTING.md` 與行為準則。
- [ ] 建立 CI：Rust format / Clippy / tests、前端 typecheck / build / tests。
- [ ] 啟用 Dependabot 或 Renovate、CodeQL、secret scanning 與 branch protection。
- [ ] 執行並發布 [OpenSSF Scorecard](https://scorecard.dev/) 結果後，再加入 Scorecard 徽章。
- [ ] 在 [OpenSSF Best Practices](https://www.bestpractices.dev/) 登記專案、誠實完成 Passing 問卷後，再加入認證徽章。
- [ ] 若提供容器映像，再加入 SBOM、簽章與可重現版本發布流程。

> 不建議現在顯示 CI passing、coverage、OpenSSF 或 Best Practices 徽章：目前 repository 沒有對應的公開結果，徽章會失真或直接顯示 unknown。

### Cua saved-login integration

`COMPUTER_IMAGE=lazyboy/computer:cua-work scripts/cua-login-test.sh` starts a disposable desktop, installs a test-only certificate utility, trusts a generated localhost certificate inside that container, and opens the HTTPS fixture through Cua. It runs the same field-filling function as `use_saved_login`, checks both exact fixture values, and verifies that the form was not submitted. No model key or real login is used; the container is removed on exit. Build the desktop image from the current tree first.

Cua 0.23.2 can return `effect: refused` for an Email field despite a zero CLI exit status. The adapter treats that as a failure. A classified unsupported browser typing route can use a uniquely labelled native web field from Cua, click its freshly observed bounds, and paste through the Cua-operated clipboard editor. Other errors remain errors.
