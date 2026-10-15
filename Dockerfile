ARG DESKTOP_IMAGE=lazyhermes-desktop:local
# Hermes and its Python are built here and copied over as one layer, after their debug symbols are stripped:
# stripping in the final image would keep both copies.
FROM ${DESKTOP_IMAGE} AS hermes
ENV UV_PYTHON_INSTALL_DIR=/opt/python UV_CACHE_DIR=/tmp/uv-cache
WORKDIR /opt/hermes
COPY vendor/hermes-agent/ /opt/hermes/
RUN uv sync --frozen --no-dev --python 3.14 --extra mcp --extra computer-use --extra sms --extra ddgs --extra anthropic \
    && uv cache clean
# Hermes itself stays on its lock file; the data libraries for the bot's own scripts follow
# upstream whenever `make box-latest` changes BOX_REFRESH.
ARG BOX_REFRESH=pinned
RUN uv pip install --python /opt/hermes/.venv/bin/python openpyxl pdfplumber python-docx pandas && uv cache clean
RUN --mount=type=secret,id=debian_mirror apt-mirror on && apt-get update && apt-get install -y --no-install-recommends binutils \
    && find /opt/hermes/.venv /opt/python -name '*.so*' -type f -exec strip --strip-unneeded {} \; \
    && PYTHONPATH=/opt/hermes /opt/hermes/.venv/bin/python -c "import numpy, pandas, PIL, pillow_heif, cryptography, lxml, pdfplumber, openpyxl, docx, run_agent, tools.computer_use_tool, tui_gateway.ws"

FROM ${DESKTOP_IMAGE}
COPY --from=hermes /opt/python /opt/python
COPY --from=hermes /opt/hermes /opt/hermes
ENV UV_PYTHON_INSTALL_DIR=/opt/python UV_CACHE_DIR=/opt/uv-cache
RUN --mount=type=secret,id=debian_mirror mkdir -p /opt/uv-cache && chmod 1777 /opt/uv-cache && apt-mirror on && apt-get update \
    && apt-get install -y --no-install-recommends sudo && apt-mirror off && rm -rf /var/lib/apt/lists/*
ENV PATH="/opt/hermes/.venv/bin:${PATH}" HOME=/home/box HERMES_HOME=/hermes BROWSER_CDP_URL=http://127.0.0.1:9222 \
    TERMINAL_CWD=/workspace PYTHONUNBUFFERED=1 PYTHONDONTWRITEBYTECODE=1
COPY box/hermes_host.py box/desktop_mcp.py box/product_mcp.py /opt/lazyhermes/
COPY --chmod=755 box/start-bot.sh /usr/local/bin/start-bot.sh
COPY --chmod=755 box/start-desktop.sh /usr/local/bin/start-desktop.sh
EXPOSE 8642 6080
WORKDIR /workspace
CMD ["/usr/local/bin/start-bot.sh"]
