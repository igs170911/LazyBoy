ARG DESKTOP_IMAGE=lazyhermes-desktop:local
FROM ${DESKTOP_IMAGE}
COPY --from=ghcr.io/astral-sh/uv:0.12.23 /uv /uvx /usr/local/bin/
ENV UV_PYTHON_INSTALL_DIR=/opt/python UV_CACHE_DIR=/opt/uv-cache
WORKDIR /opt/hermes
COPY vendor/hermes-agent/ /opt/hermes/
RUN uv sync --frozen --no-dev --python 3.14 --extra mcp --extra computer-use --extra sms --extra ddgs --extra anthropic \
    && uv cache clean
RUN apt-get update && apt-get install -y --no-install-recommends sudo && rm -rf /var/lib/apt/lists/*
ENV PATH="/opt/hermes/.venv/bin:${PATH}" HOME=/home/box HERMES_HOME=/hermes BROWSER_CDP_URL=http://127.0.0.1:9222 \
    TERMINAL_CWD=/workspace PYTHONUNBUFFERED=1 PYTHONDONTWRITEBYTECODE=1
COPY box/hermes_host.py box/desktop_mcp.py box/product_mcp.py /opt/lazyhermes/
COPY --chmod=755 box/start-bot.sh /usr/local/bin/start-bot.sh
COPY --chmod=755 box/start-desktop.sh /usr/local/bin/start-desktop.sh
EXPOSE 8642 6080
WORKDIR /workspace
CMD ["/usr/local/bin/start-bot.sh"]
