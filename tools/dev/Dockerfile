# syntax=docker/dockerfile:1.7
#
# Neon Law Navigator devcontainer — bundles the dev-time prerequisites
# (kind, kubectl, docker CLI, the pinned Rust toolchain) so a
# contributor can drive `make dev-up` without installing anything
# on the host. Mount the host's docker socket at run time:
#
#     docker build -t navigator-devx:dev -f tools/dev/Dockerfile .
#     docker run --rm -it \
#         -v /var/run/docker.sock:/var/run/docker.sock \
#         -v "$PWD":/work \
#         -w /work \
#         --network host \
#         navigator-devx:dev \
#         make dev-up
#
# `--network host` is what lets the host browser reach the
# port-forwards started inside the container. On Docker Desktop /
# OrbStack you can drop `--network host` and rely on KIND's own
# host-port forwarding (Keycloak :30080, fake-gcs :30443) plus
# explicit `-p` flags for the rest.

FROM rust:1.96-bookworm

ARG KIND_VERSION=v0.24.0
ARG KUBECTL_VERSION=v1.31.0

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl jq make musl-tools docker.io \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add x86_64-unknown-linux-musl \
    && rustup component add rustfmt clippy

# kind
RUN curl -fsSL -o /usr/local/bin/kind \
        "https://kind.sigs.k8s.io/dl/${KIND_VERSION}/kind-linux-amd64" \
    && chmod +x /usr/local/bin/kind

# kubectl
RUN curl -fsSL -o /usr/local/bin/kubectl \
        "https://dl.k8s.io/release/${KUBECTL_VERSION}/bin/linux/amd64/kubectl" \
    && chmod +x /usr/local/bin/kubectl

WORKDIR /work
CMD ["bash"]
