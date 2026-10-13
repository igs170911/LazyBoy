.DEFAULT_GOAL := help

.PHONY: help install start stop get-access-token providers box box-latest clean build up down logs docker-token docker-import

help:
	@printf '%s\n' \
		'make install           安裝缺少的系統套件與所有依賴，並建置 Bot 電腦映像（第一次安裝與更新都用這個）' \
		'make start             背景啟動 LazyBoy' \
		'make stop              停止 LazyBoy' \
		'make get-access-token  顯示其他裝置使用的登入碼' \
		'make box               重建 Bot 電腦映像（自動選最快的 Debian 鏡像站，沿用快取），完成後到「設定 → 更新電腦」套用' \
		'make box-latest        同上，但 grok 等工具與 Python 套件更新到最新版' \
		'make providers         從 Bot 映像的 Hermes 重新匯出內建供應商清單' \
		'make clean             清掉用不到的 Docker 建置快取與無名映像（不動 Bot 的資料）' \
		'' \
		'在 Docker 中執行（主機只需要 Docker 與 git）：' \
		'make build             先確認建置平台（linux/amd64 或 linux/arm64），再建置 LazyBoy 與 Bot 電腦的映像' \
		'make up                在 Docker 中啟動 LazyBoy，並顯示含登入碼的網址' \
		'make down              停止 Docker 中的 LazyBoy（資料留在 lazyboy-data volume）' \
		'make logs              查看 Docker 中 LazyBoy 的日誌' \
		'make docker-token      顯示 Docker 中 LazyBoy 的登入碼' \
		'make docker-import     把 make install 版本的資料（.data）複製到 Docker volume，改用 Docker 執行'

# Missing system packages first (sudo asks when needed), then dependencies and images; safe to run again.
install:
	@./scripts/install.sh

# The bot image builds FROM the desktop image; scripts/build-images.sh builds both, desktop first.
# DEBIAN_MIRROR=<url> skips the mirror speed test.
BOT_IMAGE = $${LAZYHERMES_IMAGE:-lazyhermes-bot:local}
box:
	@./scripts/build-images.sh $(BOX_ARGS)
	@printf 'grok %s\n' "$$(docker run --rm --entrypoint grok $(BOT_IMAGE) --version 2>/dev/null || echo '版本未知')"
	@printf '已重建 %s。正在執行的 Bot 電腦不受影響；到「設定 → 更新電腦」套用新映像。\n' "$(BOT_IMAGE)"
	@$(MAKE) --no-print-directory clean

# A fresh BOX_REFRESH re-runs every tool layer after it; GROK_CLI_VERSION=latest drops the pin.
box-latest:
	@$(MAKE) --no-print-directory box BOX_ARGS="--build-arg BOX_REFRESH=$$(date +%Y%m%d%H%M%S) --build-arg GROK_CLI_VERSION=$${GROK_CLI_VERSION:-latest}"

providers:
	@docker run --rm -i --entrypoint /opt/hermes/.venv/bin/python -w /opt/hermes $${LAZYHERMES_IMAGE:-lazyhermes-bot:local} - \
		< scripts/export-hermes-providers.py > server/hermes_providers.json.new
	@mv server/hermes_providers.json.new server/hermes_providers.json
	@printf '已更新 server/hermes_providers.json\n'

# Build cache beyond CACHE_KEEP and untagged images are dropped; tagged images, volumes and bot data stay.
CACHE_KEEP ?= 4gb
clean:
	@docker builder prune -f --reserved-space $(CACHE_KEEP) | tail -n 1
	@docker image prune -f | tail -n 1

start:
	@./scripts/start-background.sh

stop:
	@./scripts/stop.sh

get-access-token:
	@if [ -n "$${LAZYHERMES_ACCESS_TOKEN:-}" ]; then \
		printf '%s\n' "$$LAZYHERMES_ACCESS_TOKEN"; \
	elif [ -f "$${LAZYHERMES_DATA_DIR:-.data}/access-token" ]; then \
		cat "$${LAZYHERMES_DATA_DIR:-.data}/access-token"; \
	else \
		printf '%s\n' '找不到登入碼，請先執行 make start，或確認 LAZYHERMES_DATA_DIR。' >&2; \
		exit 1; \
	fi

# LazyBoy in Docker (compose.yaml). PLATFORM=linux/arm64 builds for another platform than this Docker's own.
build:
	@./scripts/build-images.sh --server $(BOX_ARGS)
	@$(MAKE) --no-print-directory clean

up:
	@./scripts/docker-up.sh

# Bot computers belong to the service: they stop with it and start again when a bot gets work.
down:
	@ids=$$(docker ps -q --filter label=lazyhermes.managed=true --filter label=lazyhermes.storage=volume); \
		if [ -n "$$ids" ]; then docker stop -t 20 $$ids >/dev/null; fi
	@docker compose down
	@printf 'LazyBoy 已停止。資料保留在 Docker volume %s。\n' "$${LAZYBOY_VOLUME:-lazyboy-data}"

logs:
	@docker compose logs -f --tail 100

docker-token:
	@docker exec $${LAZYBOY_CONTAINER:-lazyboy} cat /data/access-token

docker-import:
	@./scripts/docker-import.sh
