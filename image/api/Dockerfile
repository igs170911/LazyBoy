FROM node:22-bookworm-slim AS web
WORKDIR /src/apps/web
COPY apps/web/package.json apps/web/package-lock.json ./
RUN npm ci
COPY apps/web/index.html apps/web/tsconfig.json apps/web/vite.config.ts ./
COPY apps/web/src src
RUN npm run build

FROM rust:1-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY migrations migrations
COPY apps/web apps/web
RUN cargo build --release -p lazyboy-api

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates python3 python3-pip python3-lxml \
    && pip3 install --break-system-packages --no-cache-dir mcp-server-fetch \
    && rm -rf /var/lib/apt/lists/*
COPY --from=web /usr/local/bin/node /usr/local/bin/node
COPY --from=web /usr/local/lib/node_modules/npm /usr/local/lib/node_modules/npm
COPY --from=build /src/target/release/lazyboy-api /usr/local/bin/lazyboy-api
COPY --from=web /src/apps/web/dist /web
RUN ln -sf /usr/local/lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -sf /usr/local/lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx \
    && npm install -g --omit=dev \
        @modelcontextprotocol/server-memory \
        @modelcontextprotocol/server-sequential-thinking \
        @notionhq/notion-mcp-server \
    && npm cache clean --force \
    && node -v && npx --version \
    && python3 -c "import mcp_server_fetch"
ENV LAZYBOY_WEB_DIR=/web
ENV NPM_CONFIG_UPDATE_NOTIFIER=false
ENV NPM_CONFIG_FUND=false
EXPOSE 3100
CMD ["lazyboy-api"]
