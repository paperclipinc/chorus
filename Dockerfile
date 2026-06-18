FROM rust:1-slim AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release -p chorus-server

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /app/target/release/chorus-server /usr/local/bin/chorus-server
USER nonroot
ENTRYPOINT ["/usr/local/bin/chorus-server"]
