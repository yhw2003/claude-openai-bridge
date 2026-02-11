# syntax=docker/dockerfile:1.6

# Build a small, fully-static binary (musl) and run it in scratch.
FROM --platform=$BUILDPLATFORM rust:1.93-alpine AS builder

RUN apk add --no-cache musl-dev build-base ca-certificates
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /app

# 1) Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && printf 'fn main() {}\n' > src/main.rs
RUN cargo build --release --locked --target x86_64-unknown-linux-musl

# 2) Build the real binary
COPY src ./src
RUN find src -type f -exec touch {} + \
    && cargo build --release --locked --target x86_64-unknown-linux-musl
RUN strip target/x86_64-unknown-linux-musl/release/claude-openai-bridge

# Minimal runtime image
FROM scratch
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/claude-openai-bridge /claude-openai-bridge
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt

EXPOSE 8082
USER 10001:10001
ENTRYPOINT ["/claude-openai-bridge"]
