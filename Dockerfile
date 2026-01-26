# syntax=docker/dockerfile:1

FROM rust:1.88-bookworm AS builder

WORKDIR /app

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    ca-certificates \
    pkg-config \
    libssl-dev \
  && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./

RUN mkdir -p src \
  && printf '%s\n' 'fn main() {}' > src/main.rs

RUN cargo build --release --locked

COPY src ./src

RUN cargo build --release --locked


FROM debian:bookworm-slim AS runtime

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
  && rm -rf /var/lib/apt/lists/*

RUN useradd --system --uid 10001 --create-home --home-dir /home/rota --shell /usr/sbin/nologin rota

COPY --from=builder /app/target/release/rota /usr/local/bin/rota

USER rota

EXPOSE 8000 8001

ENTRYPOINT ["rota"]
