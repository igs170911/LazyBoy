.DEFAULT_GOAL := help

.PHONY: help start stop get-access-token providers box box-latest clean

help:
	@printf '%s\n' \
		'make start             背景啟動 LazyBoy' \
		'make stop              停止 LazyBoy' \
		'make get-access-token  顯示其他裝置使用的登入碼' \
		'make box               重建 Bot 電腦映像（固定版本，沿用快取），完成後到「設定 → 更新電腦」套用' \
		'make box-latest        同上，但 grok 等工具與 Python 套件更新到最新版' \
		'make providers         從 Bot 映像的 Hermes 重新匯出內建供應商清單' \
		'make clean             清掉用不到的 Docker 建置快取與無名映像（不動 Bot 的資料）'

# The bot image builds FROM the desktop image, so the desktop goes first.
BOT_IMAGE = $${LAZYHERMES_IMAGE:-lazyhermes-bot:local}
box:
	@./box/build.sh lazyhermes-desktop:local --build-arg DEBIAN_MIRROR=$(DEBIAN_MIRROR) $(BOX_ARGS)
	@docker build --build-arg DESKTOP_IMAGE=lazyhermes-desktop:local $(BOX_ARGS) -t $(BOT_IMAGE) .
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
