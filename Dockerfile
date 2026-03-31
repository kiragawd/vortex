# ─── Stage 1: Builder ───────────────────────────────────────────────────────
FROM rust:latest AS builder

# Install protobuf compiler and Python dev headers (needed for pyo3)
RUN apt-get update && apt-get install -y --no-install-recommends \
    protobuf-compiler \
    python3-dev \
    libpython3-dev \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Cache dependencies by copying only manifests first
COPY Cargo.toml Cargo.lock* ./
COPY build.rs ./
COPY proto/ proto/

# Create stub source files so cargo can resolve the dependency graph
RUN mkdir -p src/bin && \
    echo "fn main() {}" > src/main.rs && \
    echo "" > src/lib.rs && \
    echo "fn main() {}" > src/bin/vortex-cli.rs

ENV PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1

# Build dependencies only (cached layer)
RUN cargo build --release 2>/dev/null || true
RUN rm -rf src/

# Copy actual source code
COPY src/ src/
COPY migrations/ migrations/
COPY assets/ assets/
COPY python/ python/
COPY dags/ dags/

# Touch main files to invalidate cache for source changes only
RUN touch src/main.rs src/lib.rs src/bin/vortex-cli.rs

# Full release build
RUN cargo build --release --bin vortex --bin vortex-cli

# ─── Stage 2: Runtime ──────────────────────────────────────────────────────
FROM debian:trixie-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    python3 \
    python3-pip \
    libpython3-dev \
    tini \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r vortex && useradd -r -g vortex -d /app -s /bin/bash vortex

WORKDIR /app

# Copy binaries from builder
COPY --from=builder /app/target/release/vortex /usr/local/bin/vortex
COPY --from=builder /app/target/release/vortex-cli /usr/local/bin/vortex-cli

# Copy runtime assets
COPY migrations/ /app/migrations/
COPY assets/ /app/assets/
COPY python/ /app/python/
COPY dags/ /app/dags/
COPY prometheus.yml /app/prometheus.yml

# Create directories for data, logs, plugins
RUN mkdir -p /app/data /app/logs /app/plugins /app/secrets && \
    chown -R vortex:vortex /app

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:3000/api/health || exit 1

# Expose ports: HTTP API/Web UI, gRPC swarm, Prometheus metrics
EXPOSE 3000 50051 9090

USER vortex

ENTRYPOINT ["tini", "--"]
CMD ["vortex"]
