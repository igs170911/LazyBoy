.DEFAULT_GOAL := help

.PHONY: help start stop get-access-token providers

help:
	@printf '%s\n' \
		'make start             背景啟動 LazyBoy' \
		'make stop              停止 LazyBoy' \
		'make get-access-token  顯示其他裝置使用的登入碼' \
		'make providers         從 Bot 映像的 Hermes 重新匯出內建供應商清單'

providers:
	@docker run --rm -i --entrypoint /opt/hermes/.venv/bin/python -w /opt/hermes $${LAZYHERMES_IMAGE:-lazyhermes-bot:local} - \
		< scripts/export-hermes-providers.py > server/hermes_providers.json.new
	@mv server/hermes_providers.json.new server/hermes_providers.json
	@printf '已更新 server/hermes_providers.json\n'

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
