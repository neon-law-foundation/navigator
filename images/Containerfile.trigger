# syntax=docker/dockerfile:1.7
#
# Shared trigger image — one thin CronJob entrypoint, parameterized by the
# crate whose `trigger` binary it ships. Build with `--build-arg CRATE=…`:
#   archives          → starts the `Archives` nightly-export workflow
#   statutes          → starts the `Statutes` weekly-scrape workflow
#   billing-workflows → starts the `BillingCanary` workflow
#
# Each trigger POSTs to the Restate ingress to start one workflow
# invocation, then exits — the workflows themselves run on the
# `workflows-service` worker, not in these pods. Built as a static musl
# binary; runs on `gcr.io/distroless/cc` because reqwest's TLS needs the
# dynamic loader. The whole workspace is copied (the build context is the
# repo root) so the same Containerfile builds any crate's `trigger` bin.

FROM rust:1.96-bookworm AS builder

# Which crate's `trigger` binary to build. Required — no sensible default.
ARG CRATE
RUN test -n "$CRATE" \
    || (echo "build-arg CRATE is required (archives|statutes|billing-workflows)" && false)

# Which trigger bin within the crate. Defaults to `trigger` (the canary /
# archives / statutes entrypoint); billing-workflows also ships
# `reconcile-trigger`, selected via `--build-arg BIN=reconcile-trigger`.
ARG BIN=trigger

RUN apt-get update \
    && apt-get install -y --no-install-recommends musl-tools pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src

# `rust-toolchain.toml` must land BEFORE `rustup target add`: the override
# pins a specific 1.96.0 toolchain that rustup re-syncs the first time cargo
# runs in this dir. Adding the target beforehand (against the base image's
# default toolchain) leaves the re-synced toolchain without the musl std,
# and `cargo build --target …-musl` then fails with `can't find crate for
# std`. Adding it here attaches the target to the toolchain the build uses.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
RUN rustup target add x86_64-unknown-linux-musl
COPY rules             rules
COPY store             store
COPY repos             repos
COPY cli               cli
COPY web               web
COPY views             views
COPY README.md         README.md
COPY telemetry         telemetry
COPY forms             forms
COPY workflows         workflows
COPY workflows-service workflows-service
COPY cloud             cloud
COPY live-inquiry      live-inquiry
COPY mcp               mcp
COPY features          features
COPY lsp               lsp
COPY pdf               pdf
COPY templates templates
COPY archives          archives
COPY import            import
COPY statutes          statutes
COPY billing           billing
COPY billing-workflows billing-workflows

RUN cargo build --release --target x86_64-unknown-linux-musl -p "${CRATE}" --bin "${BIN}" \
    && cp "target/x86_64-unknown-linux-musl/release/${BIN}" /trigger-bin

# ---------- Stage 2: runtime ----------

FROM gcr.io/distroless/cc-debian12:nonroot AS runtime

WORKDIR /app

COPY --from=builder /trigger-bin /app/trigger

ENV RUST_LOG=info

# Identify the release. The daily `deploy.yml` passes `--build-arg
# RELEASE_TAG=$YY.M.D`; `telemetry::init` reads `NAVIGATOR_RELEASE_TAG`
# and tags every span/metric/log with `service.version`, so each trigger
# run self-reports which release fired it. A local build reports `unknown`.
ARG RELEASE_TAG=unknown
ENV NAVIGATOR_RELEASE_TAG=$RELEASE_TAG

USER nonroot:nonroot

ENTRYPOINT ["/app/trigger"]
