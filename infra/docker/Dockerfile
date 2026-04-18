# Multi-stage build: produce a tiny image containing only the CLI.
# Useful for CI pipelines where developers want to pipe code → ContextOS → LLM.
FROM rust:1.77-slim AS build
WORKDIR /work
RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential pkg-config \
 && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release --bin contextos

FROM debian:bookworm-slim
COPY --from=build /work/target/release/contextos /usr/local/bin/contextos
ENTRYPOINT ["/usr/local/bin/contextos"]
CMD ["--help"]
