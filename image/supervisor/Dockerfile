# syntax=docker/dockerfile:1
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY migrations migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,id=lazyboy-supervisor-release,sharing=locked \
    cargo build --locked --release -p lazyboy-supervisor && cp /src/target/release/lazyboy-supervisor /lazyboy-supervisor

FROM gcr.io/distroless/cc-debian12:latest
COPY --from=build /lazyboy-supervisor /usr/local/bin/lazyboy-supervisor
EXPOSE 7091
CMD ["/usr/local/bin/lazyboy-supervisor"]
