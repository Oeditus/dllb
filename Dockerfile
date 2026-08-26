# Multi-stage Dockerfile for dllb DBMS

# --- Build Stage ---
FROM rust:slim-bookworm AS builder
WORKDIR /usr/src/dllb

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    build-essential \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --release --workspace

# --- Runtime Stage ---
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    netcat-openbsd \
    libgcc-s1 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -g 10001 dllb && \
    useradd -u 10001 -g dllb -s /bin/false -m dllb

COPY --from=builder /usr/src/dllb/target/release/dllb-server /usr/local/bin/
COPY --from=builder /usr/src/dllb/target/release/dllb-cli /usr/local/bin/

ENV DLLB_BIND="0.0.0.0:3009" \
    DLLB_PATH="/var/lib/dllb/dllb.redb" \
    DLLB_NS="default" \
    DLLB_DB="default" \
    RUST_LOG="info"

RUN mkdir -p /var/lib/dllb && chown -R dllb:dllb /var/lib/dllb
VOLUME /var/lib/dllb

USER dllb:dllb
EXPOSE 3009

STOPSIGNAL SIGTERM

ENTRYPOINT ["/usr/local/bin/dllb-server"]
