# Simple VM test

This is a simple VM setup intendent to test `gvltctl build`.

Is has pre-compiled Linux kernel v6.12 (`bzImage`) and "Hello world" application (`testapp`).

## Running test

1. Compile gvltctl

    ```shell
    cargo build
    ```

2. Build VM using assets in this directory

    ```shell
    cargo run -- build \
        --containerfile Containerfile \
        --kernel-file bin/bzImage
    ```

    To simplify things we use pre-compiled kernel `bin/bzImage` here.

3. Run VM

    ```shell
    cargo run -- local-run disk.img \
        --file task.yaml \
        --input inputs/input.txt:input.txt \
        --stdout \
        --stderr \
        --smp 1 \
        --mem 512
    ```

    You should see "Hello, world!" message in the output.
