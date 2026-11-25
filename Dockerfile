# Build from the repository root: docker build -t spbftuit .
FROM rust:1-bookworm AS builder
WORKDIR /app/backend
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src ./src
COPY backend/migrations ./migrations
RUN cargo build --release --bin smart-campus-backend

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/backend/target/release/smart-campus-backend /usr/local/bin/spbftuit
ENV PORT=8080 RUST_LOG=info
EXPOSE 8080
CMD ["/usr/local/bin/spbftuit"]
