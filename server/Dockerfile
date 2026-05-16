FROM rust:1.77-slim AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock* ./
COPY server/ ./server/
COPY shared/ ./shared/

RUN cargo build --release -p server

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/server /usr/local/bin/server
EXPOSE 25919
EXPOSE 25920
CMD ["server"]
