FROM rust:1.88-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked && \
    cp /build/target/release/mission-control /mission-control

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /mission-control /usr/local/bin/mission-control
COPY config.toml events.jsonl ./
COPY tasks.txt ./
COPY souls ./souls
RUN mkdir -p /data /workspace /app/logs
ENV MISSION_CONTROL_DB=/data/mission_control.sqlite3
ENV MISSION_CONTROL_WORKSPACE_ROOT=/workspace
ENV OLLAMA_BASE_URL=http://host.docker.internal:11434
CMD ["mission-control"]
