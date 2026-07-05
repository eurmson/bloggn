# Stage 1: Build the application
FROM rust:1.85-slim-bookworm AS builder

# Install system dependencies needed for compiling SQLite and downloading assets
RUN apt-get update && apt-get install -y \
    curl \
    pkg-config \
    libsqlite3-dev \
    git \
    && rm -rf /var/lib/apt/lists/*

# Detect build platform architecture and download the matching standalone Tailwind CSS v4 CLI
RUN ARCH=$(uname -m) && \
    if [ "$ARCH" = "x86_64" ]; then \
        echo "Downloading Tailwind CLI for x86_64..." && \
        curl -sL -o /usr/local/bin/tailwindcss https://github.com/tailwindlabs/tailwindcss/releases/latest/download/tailwindcss-linux-x64; \
    elif [ "$ARCH" = "aarch64" ]; then \
        echo "Downloading Tailwind CLI for aarch64..." && \
        curl -sL -o /usr/local/bin/tailwindcss https://github.com/tailwindlabs/tailwindcss/releases/latest/download/tailwindcss-linux-arm64; \
    else \
        echo "Unsupported architecture: $ARCH" && exit 1; \
    fi && \
    chmod +x /usr/local/bin/tailwindcss

WORKDIR /usr/src/bloggn

# Copy the application code
COPY . .

# Set environment variables for the Rust compilation phase
ENV TAILWIND_CLI_PATH=/usr/local/bin/tailwindcss
ENV DATABASE_URL=diesel.db

# Build the release binary
RUN cargo build --release

# Strip debugging symbols from the binary to make it as small as possible
RUN strip target/release/bloggn

# Stage 2: Minimal runtime image
FROM debian:bookworm-slim AS runner

# Install minimal runtime packages (SQLite library and CA Certificates for webauthn/TLS)
RUN apt-get update && apt-get install -y \
    libsqlite3-0 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the compiled binary and templates (templates are loaded at runtime)
COPY --from=builder /usr/src/bloggn/target/release/bloggn /app/bloggn
COPY --from=builder /usr/src/bloggn/templates /app/templates

# Create directory to mount our persistent data volume (DB & uploads)
RUN mkdir -p /data

# Default environment variables for Rocket and the app
ENV ROCKET_ADDRESS=0.0.0.0
ENV ROCKET_PORT=8000
ENV ROCKET_PROFILE=release
ENV DATABASE_URL=/data/diesel.db
ENV IMAGE_DIR=/data/static

# Expose port 8000 for network access
EXPOSE 8000

# Run the binary
CMD ["/app/bloggn"]
