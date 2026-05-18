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
    wget \
    build-essential \
    perl \
    linux-libc-dev \
    && rm -rf /var/lib/apt/lists/*

# Add musl target
RUN rustup target add x86_64-unknown-linux-musl

# Build musl-compatible OpenSSL from source
ENV OPENSSL_VERSION=3.3.2
RUN wget https://github.com/openssl/openssl/releases/download/openssl-${OPENSSL_VERSION}/openssl-${OPENSSL_VERSION}.tar.gz \
    && tar xzf openssl-${OPENSSL_VERSION}.tar.gz \
    && cd openssl-${OPENSSL_VERSION} \
    && CC="musl-gcc -fPIE -pie" ./Configure no-shared no-async --prefix=/musl --openssldir=/musl/ssl linux-x86_64 \
    && make -j$(nproc) \
    && make install_sw \
    && cd .. \
    && rm -rf openssl-${OPENSSL_VERSION} openssl-${OPENSSL_VERSION}.tar.gz

# Point openssl-sys to the musl-compatible OpenSSL
ENV OPENSSL_DIR=/musl \
    OPENSSL_STATIC=1 \
    PKG_CONFIG_ALLOW_CROSS=1

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
