# Stage 1: Build the application
FROM rust:slim-bookworm AS builder

# Install system dependencies needed for compiling SQLite and downloading assets
RUN apt-get update && apt-get install -y \
    curl \
    pkg-config \
    libsqlite3-dev \
    libssl-dev \
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

# Set environment variables for compilation
ENV TAILWIND_CLI_PATH=/usr/local/bin/tailwindcss
ENV DATABASE_URL=diesel.db

# Build the release binary (this will run the real build.rs and generate output.css)
RUN cargo build --release

# Strip debugging symbols from the binary to make it as small as possible
RUN strip target/release/bloggn

# Stage 2: Minimal runtime image
FROM debian:bookworm-slim AS runner

# Install minimal runtime packages (SQLite library and CA Certificates for webauthn/TLS)
RUN apt-get update && apt-get install -y \
    libsqlite3-0 \
    ca-certificates \
    libssl3 \
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

# Stage 3: Test runner
FROM builder AS tester

# Ensure static directories exist as they are gitignored and needed by Rocket FileServer during tests
RUN mkdir -p static static/images

# Install system dependencies, Chromium, and Firefox
RUN apt-get update && apt-get install -y \
    chromium \
    chromium-driver \
    firefox-esr \
    && rm -rf /var/lib/apt/lists/*

# Download and install geckodriver (pinned version to avoid GitHub API rate limits in CI)
RUN ARCH=$(uname -m) && \
    if [ "$ARCH" = "x86_64" ]; then \
        GECKO_ARCH="linux64"; \
    elif [ "$ARCH" = "aarch64" ]; then \
        GECKO_ARCH="linux-aarch64"; \
    else \
        echo "Unsupported architecture: $ARCH" && exit 1; \
    fi && \
    GECKO_VER="v0.34.0" && \
    GECKODRIVER_URL="https://github.com/mozilla/geckodriver/releases/download/${GECKO_VER}/geckodriver-${GECKO_VER}-${GECKO_ARCH}.tar.gz" && \
    curl -sL -o /tmp/geckodriver.tar.gz "$GECKODRIVER_URL" && \
    tar -xzf /tmp/geckodriver.tar.gz -C /usr/local/bin && \
    rm /tmp/geckodriver.tar.gz && \
    chmod +x /usr/local/bin/geckodriver

# Set environment variables for testing
ENV CHROMEDRIVER_PATH=/usr/bin/chromedriver
ENV GECKODRIVER_PATH=/usr/local/bin/geckodriver
ENV ROCKET_SECRET_KEY=i7DJj20DP8cqraea4OhLCWY+oJKa780VhW07Jihp9oI=

# Pre-compile the tests to speed up container execution and cache dependencies
RUN cargo test --no-run --release

# Run the test suite
CMD ["cargo", "test", "--release"]
