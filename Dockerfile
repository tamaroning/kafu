# syntax=docker/dockerfile:1.7

# Stage 1: Download kafu_serve from GitHub Release.
FROM debian:bookworm-slim AS downloader

ARG KAFU_VERSION=0.1.0
ARG TARGETARCH

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates \
      curl \
      gzip \
    && rm -rf /var/lib/apt/lists/*

# Use BuildKit secret mount so the token never persists in image layers.
RUN --mount=type=secret,id=github_token \
    ARCH=$(case "${TARGETARCH}" in amd64) echo x86_64;; arm64) echo aarch64;; *) echo "${TARGETARCH}";; esac) && \
    GITHUB_TOKEN=$(cat /run/secrets/github_token 2>/dev/null || true) && \
    URL="https://github.com/tamaroning/kafu/releases/download/${KAFU_VERSION}/kafu_serve-${KAFU_VERSION}-${ARCH}-linux.gz" && \
    if [ -n "${GITHUB_TOKEN}" ]; then \
      curl -fSL -H "Authorization: token ${GITHUB_TOKEN}" "${URL}" -o /tmp/kafu_serve.gz; \
    else \
      curl -fSL "${URL}" -o /tmp/kafu_serve.gz; \
    fi && \
    gunzip /tmp/kafu_serve.gz && \
    chmod +x /tmp/kafu_serve

# Stage 2: Minimal runtime image with only the server binary.
FROM debian:bookworm-slim

COPY --from=downloader /tmp/kafu_serve /usr/local/bin/kafu_serve

CMD ["kafu_serve", "--version"]
