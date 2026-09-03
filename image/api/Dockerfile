FROM node:22-bookworm-slim AS web
WORKDIR /src/apps/web
COPY apps/web/package.json apps/web/package-lock.json ./
RUN npm ci
COPY apps/web/index.html apps/web/tsconfig.json apps/web/vite.config.ts ./
COPY apps/web/src src
RUN npm run build

FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY migrations migrations
COPY apps/web apps/web
RUN cargo build --release -p lazyboy-api

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/lazyboy-api /usr/local/bin/lazyboy-api
COPY --from=web /src/apps/web/dist /web
ENV LAZYBOY_WEB_DIR=/web
EXPOSE 3100
CMD ["lazyboy-api"]
