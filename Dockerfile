# ── Stage 1: Build the React dashboard ──
FROM node:20-alpine AS dashboard-builder
WORKDIR /app/dashboard
COPY dashboard/package.json dashboard/package-lock.json* ./
RUN npm ci
COPY dashboard/ ./
RUN npm run build

# ── Stage 2: Build the Rust binary ──
FROM rust:1.83-bookworm AS rust-builder
WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Cache dependencies by building a dummy project first
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release 2>/dev/null || true
RUN rm -rf src

# Now build the real project
COPY src/ src/
COPY migrations/ migrations/
RUN cargo build --release

# ── Stage 3: Final slim image ──
FROM debian:bookworm-slim
WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

# Copy the compiled binary
COPY --from=rust-builder /app/target/release/pretty_rusty ./pretty_rusty

# Copy the built dashboard
COPY --from=dashboard-builder /app/dashboard/dist ./dashboard/dist

# Copy migrations
COPY migrations/ migrations/

# Create data directory for SQLite
RUN mkdir -p data

# Expose the server port
EXPOSE 3001

# Run the engine
CMD ["./pretty_rusty"]
