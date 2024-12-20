FROM rust:1.81 as builder

WORKDIR /usr/src/app

COPY Cargo.toml Cargo.lock ./

COPY server/Cargo.toml server/Cargo.toml
RUN mkdir server/src && echo "fn main() {}" > server/src/main.rs
COPY docs/Cargo.toml docs/Cargo.toml
RUN mkdir docs/src && echo "fn main() {}" > docs/src/main.rs
COPY cli/Cargo.toml cli/Cargo.toml
RUN mkdir cli/src && echo "fn main() {}" > cli/src/main.rs
COPY shared/Cargo.toml shared/Cargo.toml
RUN mkdir shared/src && echo "fn main() {}" > shared/src/lib.rs

RUN cargo build --release --workspace && rm -rf target/release/deps/*app*

COPY . .

RUN cargo build --release --package server

FROM debian:bookworm-slim 

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/app/target/release/server /usr/local/bin/server

ENV RUST_LOG=info
EXPOSE 8081

CMD ["server"]

