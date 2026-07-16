# ---- Build stage: compile binaries and seed the database ----
# trixie is required: ort's prebuilt static onnxruntime needs glibc >= 2.38.
FROM rust:1-trixie AS builder
# Populated automatically by buildx (e.g. "amd64", "arm64").
ARG TARGETARCH
WORKDIR /app

COPY . .

# The repo bundles an x86-64 ./shared/libpdfium.so. For arm64 builds, replace
# it with the matching prebuilt library so the seeder (below) and the runtime
# can bind to pdfium. amd64 keeps the committed library.
RUN set -eux; \
    if [ "$TARGETARCH" = "arm64" ]; then \
        curl -fsSL -o /tmp/pdfium.tgz \
            "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7947/pdfium-linux-arm64.tgz"; \
        tar -xzf /tmp/pdfium.tgz -C /tmp lib/libpdfium.so; \
        cp /tmp/lib/libpdfium.so ./shared/libpdfium.so; \
        rm -rf /tmp/pdfium.tgz /tmp/lib; \
    fi

RUN cargo build --release -p api -p database-seeder

# Seed database.db from the PDF docs. Requires ./shared (pdfium + tokenizer);
# downloads the embedding model into .fastembed_cache unless the build
# context already contains it.
RUN ./target/release/database-seeder ./docs

# ---- Runtime stage ----
FROM debian:trixie-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends openssl ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/api ./api
COPY --from=builder /app/database.db* ./
COPY --from=builder /app/.fastembed_cache ./.fastembed_cache
COPY --from=builder /app/shared/libpdfium.so ./shared/libpdfium.so
COPY --from=builder /app/docs ./docs

ENV DOCS_ROOT=/app/docs
EXPOSE 8080

# start-period covers loading the embedding model and pdfium at startup.
HEALTHCHECK --interval=30s --timeout=3s --start-period=15s --retries=3 \
    CMD curl -fsS http://localhost:8080/health || exit 1

CMD ["./api", "http"]
