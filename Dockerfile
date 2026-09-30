# syntax=docker/dockerfile:1
# © 2026 aiaiaiai · aiaiaiai.org
# SPDX-License-Identifier: MIT

# Build. The toolchain is pinned so a rebuild of the same commit is the same compiler.
FROM rust:1.94-slim-bookworm AS build
WORKDIR /src
RUN apt-get update \
    && apt-get install --yes --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --locked -p h-ua-bot

# Run. No shell, no package manager, and not root.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/h-ua-bot /usr/local/bin/h-ua-bot

# 1927 is the port infra gives every production workload. GET /health answers on it without a
# secret; POST /api/v1/delivery is where prism-hub delivers.
ENV HUA_DELIVERY_LISTEN=0.0.0.0:1927
EXPOSE 1927

ENTRYPOINT ["/usr/local/bin/h-ua-bot"]
CMD ["run"]
