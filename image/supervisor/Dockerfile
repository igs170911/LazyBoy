# syntax=docker/dockerfile:1
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY .cargo .cargo
COPY crates crates
COPY migrations migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,id=lazyboy-supervisor-release,sharing=locked \
    cargo build --locked --release -p lazyboy-supervisor && cp /src/target/release/lazyboy-supervisor /lazyboy-supervisor

# Pin the multi-arch (amd64 + arm64) index by digest: `latest` is mutable and a
# floating base silently changes the supervisor's libc on every rebuild.
FROM gcr.io/distroless/cc-debian12@sha256:e5d81ddde149641e2a9ba55be4545bc125c67de07508b03ba4c22e6eb0ded5aa
COPY --from=build /lazyboy-supervisor /usr/local/bin/lazyboy-supervisor
EXPOSE 7091
CMD ["/usr/local/bin/lazyboy-supervisor"]
