FROM rust:1-slim AS builder
# wreq compiles BoringSSL from source: needs C++ toolchain, cmake, libclang
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential cmake libclang-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /usr/src/app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/app/target/release/academia-dl /usr/local/bin/academia-dl
VOLUME /data
WORKDIR /data
ENTRYPOINT ["academia-dl"]
