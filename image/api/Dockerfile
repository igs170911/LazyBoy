# syntax=docker/dockerfile:1
FROM node:22-bookworm-slim AS web
WORKDIR /src/apps/web
COPY apps/web/package.json apps/web/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci --no-audit --no-fund
COPY apps/web/index.html apps/web/tsconfig.json apps/web/vite.config.ts ./
COPY apps/web/public public
COPY apps/web/src src
RUN npm run build \
    && apt-get update \
    && apt-get install -y --no-install-recommends binutils \
    && strip --strip-unneeded /usr/local/bin/node \
    && rm -rf /var/lib/apt/lists/*

FROM debian:trixie-slim AS python
RUN apt-get update && apt-get install -y --no-install-recommends \
        python3 python3-pip python3-lxml \
    && pip3 install --break-system-packages --no-cache-dir \
        --no-compile --target /opt/python mcp-server-fetch==2026.8.18 \
    && find /opt/python -type d -name __pycache__ -prune -exec rm -rf '{}' + \
    && rm -rf /var/lib/apt/lists/*

FROM rust:1-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY .cargo .cargo
COPY crates crates
COPY migrations migrations
COPY apps/web/vnc.html apps/web/vnc.html
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,id=lazyboy-api-release,sharing=locked \
    cargo build --locked --release -p lazyboy-api && cp /src/target/release/lazyboy-api /lazyboy-api

FROM debian:trixie-slim AS onnxruntime
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY scripts/fetch-onnxruntime.sh /fetch-onnxruntime.sh
RUN chmod +x /fetch-onnxruntime.sh \
    && TARGETARCH="${TARGETARCH}" /fetch-onnxruntime.sh /opt/onnxruntime

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates python3 python3-lxml libstdc++6 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=onnxruntime /opt/onnxruntime/lib/ /usr/local/lib/
COPY --from=web /usr/local/bin/node /usr/local/bin/node
COPY --from=web /usr/local/lib/node_modules/npm /usr/local/lib/node_modules/npm
COPY --from=python /opt/python /opt/python
COPY --from=build /lazyboy-api /usr/local/bin/lazyboy-api
COPY --from=web /src/apps/web/dist /web
COPY --from=web /src/apps/web/node_modules/@novnc/novnc/core /web/novnc/core
COPY --from=web /src/apps/web/node_modules/@novnc/novnc/vendor /web/novnc/vendor
RUN ln -sf /usr/local/lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -sf /usr/local/lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx \
    && npm install -g --omit=dev \
        @modelcontextprotocol/server-memory@2026.8.31 \
        @modelcontextprotocol/server-sequential-thinking@2026.8.31 \
        @notionhq/notion-mcp-server@2.5.1 \
    && npm cache clean --force \
    && node -v && npx --version \
    && PYTHONPATH=/opt/python python3 -c "import mcp_server_fetch"
ENV LAZYBOY_WEB_DIR=/web \
    PYTHONPATH=/opt/python \
    ORT_DYLIB_PATH=/usr/local/lib/libonnxruntime.so
ENV NPM_CONFIG_UPDATE_NOTIFIER=false
ENV NPM_CONFIG_FUND=false
RUN useradd --create-home --uid 1000 lazyboy
USER 1000:1000
EXPOSE 3100
CMD ["lazyboy-api"]
