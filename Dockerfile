# syntax=docker/dockerfile:1

############################
# Builder
############################
FROM rust:1.91-bookworm AS builder
WORKDIR /app

# Improve build caching
COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs
RUN cargo build --release

# Real sources
RUN rm -rf src
COPY src ./src
RUN cargo build --release

############################
# Runtime
############################
FROM node:20-bookworm-slim AS runtime

# Allure Report 3 CLI is distributed via npm ("allure")
# Also install a JRE for compatibility with some plugins/tools.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates openjdk-17-jre-headless \
 && rm -rf /var/lib/apt/lists/* \
 && npm install -g allure \
 && allure --version

ENV DATA_DIR=/data
ENV LISTEN=0.0.0.0:8080
ENV ALLURE_BIN=allure

WORKDIR /opt/app
COPY --from=builder /app/target/release/allure3-docker-service /usr/local/bin/allure3-docker-service

EXPOSE 8080
VOLUME ["/data"]

ENTRYPOINT ["/usr/local/bin/allure3-docker-service"]
