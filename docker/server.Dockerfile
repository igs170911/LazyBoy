# LazyBoy's own service, for running LazyBoy in Docker (compose.yaml). Bot computers are separate images
# (box/Dockerfile, Dockerfile); this one only serves the web app and starts those computers through the host's
# Docker socket. Build it with `make build`, which builds the bot images first.
FROM node:24-bookworm-slim AS web
WORKDIR /app/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
COPY tools/frontend-assets.test.mjs /app/tools/
RUN npm run build

FROM python:3.14.8-slim-bookworm
COPY --from=ghcr.io/astral-sh/uv:0.12.23 /uv /usr/local/bin/
COPY --from=docker:29.7.2-cli /usr/local/bin/docker /usr/local/bin/docker
ENV UV_PROJECT_ENVIRONMENT=/opt/venv UV_PYTHON_DOWNLOADS=never UV_COMPILE_BYTECODE=1 PYTHONUNBUFFERED=1 \
    PATH=/opt/venv/bin:$PATH LAZYHERMES_DATA_DIR=/data
WORKDIR /app
COPY pyproject.toml uv.lock ./
RUN uv sync --frozen --no-dev --no-install-project && rm -rf /root/.cache
# The service runs as this user; bot computers run as the same UID, so both own the files in the data volume.
RUN useradd --uid 1000 --user-group --home-dir /data --no-create-home lazyboy && mkdir /data && chown lazyboy:lazyboy /data
COPY HERMES_VERSION ./
COPY vendor/hermes-agent/optional-mcps/ vendor/hermes-agent/optional-mcps/
COPY box/product_mcp.py box/
COPY server/ server/
COPY web/novnc-skin.html web/
COPY --from=web /app/web/dist/ web/dist/
COPY --chmod=755 docker/entrypoint.sh /usr/local/bin/lazyboy-entrypoint
EXPOSE 8790
VOLUME /data
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s \
    CMD python -c "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8790/api/health', timeout=4)"
ENTRYPOINT ["/usr/local/bin/lazyboy-entrypoint"]
CMD ["python", "-m", "server.serve"]
