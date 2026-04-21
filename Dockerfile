ARG RUST_VERSION=1.95.0
ARG APP_NAME=claude-auth-proxy

################################################################################
# BUILD STAGE
################################################################################
FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-trixie AS build

ARG TARGETPLATFORM
ARG BUILDPLATFORM
ARG APP_NAME
ARG TARGET_TRIPLE

# Install cross-compilation toolchain and build dependencies
# cmake is required for aws-lc-sys (AWS libcrypto)
RUN apt-get update && apt-get install -y \
    cmake \
    clang \
    lld \
    gcc \
    libc6-dev \
    git \
    pkg-config \
    libssl-dev

# Determine target triple and set up cross-compilation
RUN set -e; \
    if [ -n "$TARGET_TRIPLE" ]; then \
        echo "$TARGET_TRIPLE" > /tmp/target.txt; \
    else \
        case "$TARGETPLATFORM" in \
            "linux/amd64") echo "x86_64-unknown-linux-gnu" ;; \
            "linux/arm64") echo "aarch64-unknown-linux-gnu" ;; \
            *) echo "$TARGETPLATFORM" | sed 's/\//-/g' ;; \
        esac > /tmp/target.txt; \
    fi; \
    cat /tmp/target.txt

ENV TARGET_TRIPLE="$(cat /tmp/target.txt)"

# Install cross-compilation target
RUN rustup target add "$(cat /tmp/target.txt)"

# Cross-compile env vars for every supported target.
ENV CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \
    CXX_x86_64_unknown_linux_gnu=x86_64-linux-gnu-g++ \
    AR_x86_64_unknown_linux_gnu=x86_64-linux-gnu-ar \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++ \
    AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc

# Install the cross-toolchain packages for the current target.
RUN set -e; \
    TT="$(cat /tmp/target.txt)"; \
    case "$TT" in \
        "x86_64-unknown-linux-gnu")    PKGS="gcc-x86-64-linux-gnu g++-x86-64-linux-gnu libc6-dev-amd64-cross" ;; \
        "aarch64-unknown-linux-gnu")   PKGS="gcc-aarch64-linux-gnu g++-aarch64-linux-gnu libc6-dev-arm64-cross" ;; \
        *)                             PKGS="" ;; \
    esac; \
    if [ -n "$PKGS" ]; then \
        apt-get update && \
        apt-get install -y --no-install-recommends $PKGS && \
        rm -rf /var/lib/apt/lists/*; \
    fi

WORKDIR /app

# Cache deps
RUN --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
    --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=bind,source=claude-auth-providers/Cargo.toml,target=claude-auth-providers/Cargo.toml \
    --mount=type=bind,source=claude-auth-transform/Cargo.toml,target=claude-auth-transform/Cargo.toml \
    --mount=type=cache,target=/app/target/,id=cargo-target-${TARGETPLATFORM},sharing=private \
    --mount=type=cache,target=/usr/local/cargo/registry/,id=cargo-registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git/db,id=cargo-git,sharing=locked \
    TT="$(cat /tmp/target.txt)" && \
    mkdir -p src claude-auth-providers/src claude-auth-transform/src && \
    echo 'fn main() {}' > src/main.rs && \
    echo 'fn main() {}' > claude-auth-providers/src/lib.rs && \
    echo 'fn main() {}' > claude-auth-transform/src/lib.rs && \
    cargo build --locked --release --target "$TT" 2>/dev/null || true

# Actually build
RUN --mount=type=bind,source=src,target=src \
    --mount=type=bind,source=claude-auth-providers/src,target=claude-auth-providers/src \
    --mount=type=bind,source=claude-auth-transform/src,target=claude-auth-transform/src \
    --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
    --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=bind,source=claude-auth-providers/Cargo.toml,target=claude-auth-providers/Cargo.toml,ro=false \
    --mount=type=bind,source=claude-auth-transform/Cargo.toml,target=claude-auth-transform/Cargo.toml,ro=false \
    --mount=type=cache,target=/app/target/,id=cargo-target-${TARGETPLATFORM},sharing=private \
    --mount=type=cache,target=/usr/local/cargo/registry/,id=cargo-registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git/db,id=cargo-git,sharing=locked \
    TT="$(cat /tmp/target.txt)" && \
    cargo build --locked --release --target "$TT" && \
    cp "/app/target/$TT/release/$APP_NAME" /bin/server

################################################################################
# CLAUDE CLI DOWNLOAD STAGE
################################################################################
FROM --platform=$BUILDPLATFORM debian:trixie-slim AS claude-download

ARG TARGETPLATFORM
ARG CLAUDE_VERSION=stable

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        curl ca-certificates jq && \
    rm -rf /var/lib/apt/lists/*

RUN set -e; \
    case "$TARGETPLATFORM" in \
        "linux/amd64") CLAUDE_PLATFORM="linux-x64" ;; \
        "linux/arm64") CLAUDE_PLATFORM="linux-arm64" ;; \
        *) echo "Unsupported platform for Claude CLI: $TARGETPLATFORM" >&2; exit 1 ;; \
    esac; \
    BASE="https://downloads.claude.ai/claude-code-releases"; \
    VERSION="$CLAUDE_VERSION"; \
    if [ "$VERSION" = "latest" ] || [ "$VERSION" = "stable" ]; then \
        VERSION=$(curl -fsSL "$BASE/$VERSION"); \
    fi; \
    echo "Installing Claude CLI $VERSION for $CLAUDE_PLATFORM"; \
    CHECKSUM=$(curl -fsSL "$BASE/$VERSION/manifest.json" | jq -er --arg p "$CLAUDE_PLATFORM" '.platforms[$p].checksum'); \
    curl -fsSL -o /claude "$BASE/$VERSION/$CLAUDE_PLATFORM/claude"; \
    echo "$CHECKSUM  /claude" | sha256sum -c -; \
    chmod 0755 /claude

################################################################################
# RUNTIME STAGE
################################################################################
FROM gcr.io/distroless/cc-debian13:nonroot AS final
USER nonroot:nonroot

COPY --from=build --chown=nonroot:nonroot /bin/server /bin/
COPY --from=claude-download --chown=nonroot:nonroot /claude /usr/local/bin/claude

EXPOSE 3000

ENTRYPOINT ["/bin/server"]
CMD ["run"]
