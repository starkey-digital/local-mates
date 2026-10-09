# Build from the repo root (the server is a workspace member):
#   docker build -f server/Dockerfile .
FROM rust:1.93-slim AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p mates-server

FROM debian:trixie-slim
COPY --from=build /src/target/release/mates-server /usr/local/bin/mates-server
USER 65532:65532
EXPOSE 8080/tcp
ENTRYPOINT ["mates-server"]
