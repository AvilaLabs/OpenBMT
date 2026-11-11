# The OpenBMT Box as a container image. Build from the repository root:
#   docker build -f packaging/box/Dockerfile .

# rust 1.89 on Debian trixie, slim. The workspace's minimum Rust is 1.89.
# Tag read 2026-11-11 (UTC) from the Docker Hub registry; the digest is what
# is built from, the tag only says what it was.
FROM rust:1.89-slim-trixie@sha256:8cffb8fe4e8a95cf0d6a2060375e5a28aff4c752155aa9f1f9193530769bdf66 AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --locked --release -p openbmt-box

# Debian trixie, slim. Tag debian:trixie-slim read 2026-11-11 (UTC) from the
# Docker Hub registry.
FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2

# curl is not used yet. It is installed now because the Box reaches the POS
# vendors over HTTPS once their connectors are added; ca-certificates is what
# that needs.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 1000 box \
    && useradd --system --uid 1000 --gid 1000 --no-create-home \
        --home-dir /nonexistent --shell /usr/sbin/nologin box \
    && mkdir /data \
    && chown 1000:1000 /data \
    && chmod 700 /data

COPY --from=build /src/target/release/openbmt-box /usr/local/bin/openbmt-box

USER 1000:1000
VOLUME /data

# The address the Box listens on inside the container. Which address of the
# host the port is published on is set outside, by the run command.
ENV OPENBMT_BOX_BIND=0.0.0.0:8986
EXPOSE 8986

# The Box reads its console from standard input and stops when that ends, so
# the container must be started with standard input open (docker run -i,
# compose stdin_open). An invitation is typed there, or given with
# --invitation-file. The Box handles no SIGTERM of its own; start the
# container with an init (docker run --init, compose init) so that a stop is
# delivered as it should be.
ENTRYPOINT ["/bin/sh", "-c", "exec /usr/local/bin/openbmt-box /data \"$OPENBMT_BOX_BIND\" \"$@\"", "openbmt-box"]

# The Box has no health answer of its own, so the check is a TCP connect to
# its port, with bash's /dev/tcp (bash is in the slim image).
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["bash", "-c", "exec 3<>/dev/tcp/127.0.0.1/${OPENBMT_BOX_BIND##*:}"]
