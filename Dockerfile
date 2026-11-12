# syntax=docker/dockerfile:1
# engram CLI + server (UI, REST API, MCP Streamable HTTP, worker).
#   docker run -d -p 7878:7878 -v engram-data:/data -e MEMORY_TOKEN=... ghcr.io/cmjcode/engram
#   docker run -i --rm -v engram-data:/data ghcr.io/cmjcode/engram mcp --profile coding   # stdio
#   Hub tim (login Google/GitHub, memori repo bersama): lihat docker/docker-compose.yml + docker/nginx-engram.conf
FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev \
 && apt-get clean
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p memory-cli

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates tini \
 && apt-get clean \
 && useradd -r -u 10001 -d /data engram && mkdir -p /data && chown engram:engram /data
COPY --from=build /src/target/release/engram /usr/local/bin/engram
COPY --chown=engram:engram memory.toml.example /data/memory.toml.example
COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod 755 /usr/local/bin/entrypoint.sh
USER engram
WORKDIR /data
VOLUME ["/data"]
EXPOSE 7878
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s CMD ["engram", "health"]
ENV MEMORY_CONFIG=/data/memory.toml
ENTRYPOINT ["tini", "--", "entrypoint.sh"]
CMD ["serve", "--bind", "0.0.0.0:7878"]
