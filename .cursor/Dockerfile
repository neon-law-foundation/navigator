# Base image for Cursor Cloud / Background Agents working on Navigator.
#
# Bakes the native build toolchain, a pinned Rust, and a local Postgres
# (role/db `navigator`) so `cargo fmt`/`clippy`/`test --workspace` work
# out of the box. Postgres is *started* per-session in `.cursor/environment.json`
# (`start`); this Dockerfile only installs it and seeds the role/db.
#
# NOTE: Do not COPY the repo here — Cursor checks out the correct commit
# into the workspace itself.

FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

# Native build dependencies for the Rust workspace (openssl, libpq, protobuf)
# plus Postgres for the test database and the basics Cursor expects (git, curl).
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        pkg-config \
        libssl-dev \
        libpq-dev \
        protobuf-compiler \
        postgresql \
        postgresql-client \
        git \
        curl \
        ca-certificates \
        sudo \
        gnupg \
        sysvinit-utils \
    && rm -rf /var/lib/apt/lists/*

# Cursor agents run as a non-root `ubuntu` user with passwordless sudo.
RUN id -u ubuntu >/dev/null 2>&1 || useradd -m -s /bin/bash ubuntu \
    && usermod -aG sudo ubuntu \
    && echo "ubuntu ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/ubuntu \
    && chmod 0440 /etc/sudoers.d/ubuntu

# Seed the local Postgres cluster with the navigator role + database so the
# test DSN (TEST_DATABASE_URL below) works the moment the server is started.
# Matches CI: superuser `navigator` / password `navigator` / db `navigator`.
RUN service postgresql start \
    && su - postgres -c "psql -v ON_ERROR_STOP=1 -c \"CREATE ROLE navigator WITH LOGIN SUPERUSER PASSWORD 'navigator';\"" \
    && su - postgres -c "createdb -O navigator navigator" \
    && service postgresql stop

# Pinned Rust toolchain. `rust-toolchain.toml` already pins 1.95.0, so any
# `cargo` invocation in the repo uses it; we install it here so it is present
# offline and the rustfmt/clippy components are ready.
USER ubuntu
WORKDIR /home/ubuntu
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --default-toolchain 1.95.0 --profile minimal -c rustfmt -c clippy
USER root

# Make cargo/rustc available on PATH for every (non-login) shell the agent uses.
ENV PATH=/home/ubuntu/.cargo/bin:${PATH}

# Non-secret test database URL (same value CI uses). Tests create a per-run
# `test_<id>` schema against this server.
ENV TEST_DATABASE_URL=postgres://navigator:navigator@127.0.0.1:5432/navigator

USER ubuntu
