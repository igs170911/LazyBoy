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
make fmt
make clippy
make test

cd apps/web
npm run typecheck
npm run build

cd ../..
node --test tests/frontend.test.mjs
python3 tests/control.test.py
python3 tests/log-rotation.test.py

# 需要已啟動的 PostgreSQL Compose service
docker compose up -d postgres
python3 tests/retention.test.py
```

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
