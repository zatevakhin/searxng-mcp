FROM rust:1-bookworm AS builder

WORKDIR /app

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && printf 'fn main() {}\n' > src/main.rs
RUN cargo build --release --locked

# Build the real binary
COPY src ./src
RUN cargo build --release --locked


FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -r -u 10001 -g nogroup -s /usr/sbin/nologin searxng-mcp

COPY --from=builder /app/target/release/searxng-mcp /usr/local/bin/searxng-mcp

USER 10001

EXPOSE 3344

ENTRYPOINT ["/usr/local/bin/searxng-mcp"]
CMD ["--transport","streamable-http","--bind","0.0.0.0:3344"]
