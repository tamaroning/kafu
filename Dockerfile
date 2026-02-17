# syntax=docker/dockerfile:1.7

# Stage 1: Download the SDK tarball from GitHub Release and extract kafu_serve.
FROM debian:bookworm-slim AS downloader

ARG KAFU_VERSION=0.1.0
ARG TARGETARCH

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates \
      curl \
    && rm -rf /var/lib/apt/lists/*

# Use BuildKit secret mount so the token never persists in image layers.
RUN --mount=type=secret,id=github_token \
    ARCH=$(case "${TARGETARCH}" in amd64) echo x86_64;; arm64) echo aarch64;; *) echo "${TARGETARCH}";; esac) && \
    GITHUB_TOKEN=$(cat /run/secrets/github_token 2>/dev/null || true) && \
    URL="https://github.com/tamaroning/kafu/releases/download/${KAFU_VERSION}/kafu-sdk-${KAFU_VERSION}-${ARCH}-linux.tar.gz" && \
    if [ -n "${GITHUB_TOKEN}" ]; then \
      curl -fSL -H "Authorization: token ${GITHUB_TOKEN}" "${URL}" -o /tmp/sdk.tar.gz; \
    else \
      curl -fSL "${URL}" -o /tmp/sdk.tar.gz; \
    fi && \
    tar xzf /tmp/sdk.tar.gz -C /tmp && \
    cp /tmp/kafu-sdk-${KAFU_VERSION}/libexec/kafu_serve /tmp/kafu_serve && \
    chmod +x /tmp/kafu_serve

# Stage 2: Minimal runtime image with only the server binary.
FROM debian:bookworm-slim

COPY --from=downloader /tmp/kafu_serve /usr/local/bin/kafu_serve

CMD ["kafu_serve", "--version"]
