FROM rust:1.85-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --locked --release

FROM debian:bookworm-slim AS runtime

LABEL org.opencontainers.image.title="Watchtower NG"
LABEL org.opencontainers.image.description="Rust rewrite of Watchtower"
LABEL org.opencontainers.image.source="https://github.com/wiki-mod/Watch-Tower-NG"
LABEL org.opencontainers.image.licenses="MIT"

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/watchtower-rs /watchtower

EXPOSE 8080

HEALTHCHECK CMD ["/watchtower", "--health-check"]

ENTRYPOINT ["/watchtower"]
