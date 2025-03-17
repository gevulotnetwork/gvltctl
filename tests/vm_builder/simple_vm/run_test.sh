#!/bin/bash

set -eu

RUST_LOG=gvltctl=trace cargo run -- build \
    --force \
    --containerfile Containerfile \
    --kernel-file bin/bzImage

RUST_LOG=gvltctl=trace cargo run -- local-run disk.img \
    --file task.yaml \
    --input inputs/input.txt:input.txt \
    --stdout \
    --stderr \
    --smp 1 \
    --mem 512

ok=true
echo "Checking stdout..."
res=$(grep "Hello, world!" < ./output/stdout)
if [[ $res = "" ]]; then
    echo "FAILED"
    ok=false
else
    echo "OK"
fi

echo "Checking output file..."
res=$(grep "This is output." < ./output/output.txt)
if [[ $res = "" ]]; then
    echo "FAILED"
    ok=false
else
    echo "OK"
fi

if [ "$ok" = true ]; then
    echo "Tests passed"
else
    echo "Tests failed"
    exit 1;
fi
