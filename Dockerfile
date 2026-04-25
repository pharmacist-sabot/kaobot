# ---- Build Stage ----
FROM rust:1.95-slim AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Cache dependencies — copy manifests first, build a stub binary, then replace with real source.
# This layer is invalidated only when Cargo.toml or Cargo.lock changes, not on every source edit.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
    && echo 'fn main() {}' > src/main.rs \
    && cargo build --release \
    && rm -rf src

# Build the real application
COPY src ./src
# Touch main.rs so cargo knows the source changed vs the cached stub
RUN touch src/main.rs && cargo build --release

# ---- Runtime Stage ----
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Non-root user for security
RUN useradd -r -s /bin/false kaobot

COPY --from=builder /app/target/release/kaobot /usr/local/bin/kaobot

# Default log level — can be overridden via Render environment variables
ENV RUST_LOG="kaobot=info,teloxide=warn"

USER kaobot

CMD ["kaobot"]
