FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY migrations migrations
RUN cargo build --release -p lazyboy-api

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/lazyboy-api /usr/local/bin/lazyboy-api
COPY apps/web /web
ENV LAZYBOY_WEB_DIR=/web
EXPOSE 3100
CMD ["lazyboy-api"]
