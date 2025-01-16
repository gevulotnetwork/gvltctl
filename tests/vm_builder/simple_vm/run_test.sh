#!/bin/bash

set -eu

RUST_LOG=trace cargo run --features vm-builder-v2 -- build --fuse --force --containerfile ./Containerfile --no-gevulot-runtime

OUTPUT=$(qemu-system-x86_64 -machine q35 -enable-kvm -nographic --hda disk.img)
echo "$OUTPUT"

res=$(echo "$OUTPUT" | grep "Hello, world!")
if [[ $res = "" ]]; then
    echo "FAILED"
    exit 1;
else
    echo "OK"
fi
