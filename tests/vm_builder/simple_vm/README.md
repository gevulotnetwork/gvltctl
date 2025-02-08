# Simple VM test

This is a simple VM setup intendent to test `gvltctl build`.

Is has pre-compiled Linux kernel v6.12 (`bzImage`) and "Hello world" application (`testapp`).

## Running test

1. Compile gvltctl with `vm-builder-v2` feature

    ```shell
    cargo build --features vm-builder-v2
    ```

2. Build VM using assets in this directory

    ```shell
    ../../../target/debug/gvltctl build \
        --containerfile Containerfile \
        --kernel-file bin/bzImage \
        --no-gevulot-runtime
    ```

    To simplify things we use here `--no-gevulot-runtime` and `--kernel-file`.

3. Run VM with QEMU

    ```shell
    qemu-system-x86_64 -machine q35 -enable-kvm -nographic --hda disk.img
    ```

    You should see "Hello, world!" message in the output.
