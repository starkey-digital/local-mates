# Official iroh relay. The release workflow overrides the version from Cargo.lock so the relay
# always matches the iroh the app ships with. Config is mounted by the Helm chart (deploy/relay).
FROM rust:1.93-slim AS build
ARG IROH_RELAY_VERSION=1.3.0
RUN cargo install iroh-relay --version "${IROH_RELAY_VERSION}" --features server --locked

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/cargo/bin/iroh-relay /usr/local/bin/iroh-relay

# Unprivileged ports so it runs as non-root; the Service maps 80/443 to these.
USER 65532:65532
WORKDIR /var/lib/iroh-relay
EXPOSE 8080/tcp 8443/tcp 7842/udp
ENTRYPOINT ["iroh-relay"]
CMD ["--config-path", "/etc/iroh-relay/config.toml"]
