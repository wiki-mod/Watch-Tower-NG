FROM --platform=$TARGETPLATFORM rust:1.85-alpine AS builder

ARG WATCHTOWER_VERSION=dev
ENV WATCHTOWER_VERSION=${WATCHTOWER_VERSION}

RUN apk add --no-cache musl-dev

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --locked --release

FROM alpine:latest AS certs

RUN apk add --no-cache \
    ca-certificates \
    tzdata

FROM scratch

LABEL "com.centurylinklabs.watchtower"="true"

COPY --from=certs \
    /etc/ssl/certs/ca-certificates.crt \
    /etc/ssl/certs/ca-certificates.crt
COPY --from=certs \
    /usr/share/zoneinfo \
    /usr/share/zoneinfo

EXPOSE 8080

COPY --from=builder /app/target/release/watchtower-rs /watchtower

HEALTHCHECK CMD ["/watchtower", "--health-check"]

ENTRYPOINT ["/watchtower"]
