# =========================
# Build Stage
# =========================
FROM rust:1.95-slim AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    musl-tools \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Add musl target
RUN rustup target add x86_64-unknown-linux-musl

# -------------------------
# Cache dependencies
# -------------------------
COPY Cargo.toml Cargo.lock ./

RUN mkdir -p src \
    && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --target x86_64-unknown-linux-musl \
    && rm -rf src

# -------------------------
# Copy real source
# -------------------------
COPY src ./src

# Rebuild actual application
RUN touch src/main.rs \
    && cargo build --release --target x86_64-unknown-linux-musl

# =========================
# Runtime Stage
# =========================
FROM scratch

# Copy CA certificates (important for HTTPS requests)
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Copy compiled binary
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/kaobot /kaobot

# Environment variables
ENV RUST_LOG="kaobot=info,teloxide=warn"

# Run application
CMD ["/kaobot"]
