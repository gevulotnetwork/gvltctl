# Use the official Rust image as the build environment
FROM rust:1.82-bullseye as builder

RUN apt-get update && apt-get install -y \
    protobuf-compiler curl \
    && rm -rf /var/lib/apt/lists/*

RUN curl -sSL "https://github.com/bufbuild/buf/releases/download/v1.0.0/buf-Linux-x86_64" -o /usr/local/bin/buf
RUN chmod +x /usr/local/bin/buf

RUN USER=root cargo new --bin /gvltctl

WORKDIR /gvltctl

COPY Cargo.toml .
COPY Cargo.lock .

# pre-build dependencies
RUN cargo build --release

# Copy the source code and build script
COPY src ./src
COPY build.rs .

# Build the project
RUN cargo build --release && cp target/release/gvltctl /gvltctl-bin

# Use a minimal base image for the final stage
FROM debian:bullseye-slim

# Install necessary dependencies
RUN apt-get update && apt-get install -y \
    libssl1.1 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy the compiled binary from the builder stage
COPY --from=builder /gvltctl-bin /usr/local/bin/gvltctl

# Set the entrypoint
ENTRYPOINT ["/usr/local/bin/gvltctl"]
