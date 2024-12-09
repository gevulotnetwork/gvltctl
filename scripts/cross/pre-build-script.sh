#!/usr/bin/env bash

set -eu

BUF_VERSION=1.47.2
PROTOC_VERSION=25.1

export DEBIAN_FRONTEND=noninteractive

dpkg --add-architecture $CROSS_DEB_ARCH

apt-get update
apt-get install -y \
    openssl \
    ca-certificates \
    pkg-config \
    cmake \
    libssl-dev:$CROSS_DEB_ARCH \
    curl \
    unzip

curl -sSL -o /usr/local/bin/buf "https://github.com/bufbuild/buf/releases/download/v${BUF_VERSION}/buf-Linux-x86_64"
chmod +x /usr/local/bin/buf

curl -sSL -o /tmp/protoc.zip "https://github.com/protocolbuffers/protobuf/releases/download/v${PROTOC_VERSION}/protoc-${PROTOC_VERSION}-linux-x86_64.zip"
unzip /tmp/protoc.zip -d /usr/local
