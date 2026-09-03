FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY migrations migrations
RUN cargo build --release -p lazyboy-supervisor

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/lazyboy-supervisor /usr/local/bin/lazyboy-supervisor
EXPOSE 7091
CMD ["lazyboy-supervisor"]
