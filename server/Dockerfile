# Floating latest 1.x — matches CI's dtolnay/rust-toolchain@stable so
# Docker builds don't fall behind the lockfile version (Cargo.lock v4
# requires Cargo >= 1.78, and the pin had drifted to 1.77). Keep the
# `-bookworm` suffix so glibc matches the runtime base below.
FROM rust:1-slim-bookworm AS builder
WORKDIR /app

ARG RELEASE_CHANNEL=dev

COPY Cargo.toml Cargo.lock* ./
COPY server/ ./server/
COPY shared/ ./shared/

RUN RELEASE_CHANNEL=${RELEASE_CHANNEL} cargo build --release -p server

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/server /usr/local/bin/server
EXPOSE 25919
EXPOSE 25920
CMD ["server"]
