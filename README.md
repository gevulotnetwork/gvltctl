# Gevulot Control CLI

This tool is used to interact with Gevulot Network.

## Installation

### Pre-built releases

You can download pre-built release binaries from [releases](https://github.com/gevulotnetwork/gvltctl/releases):

Supported platforms:

- `x86_64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

#### Installation of pre-built release

1. Download archive

    ```shell
    curl -fLO https://github.com/gevulotnetwork/gvltctl/releases/download/${VERSION}/gvltctl-${PLATFORM}.tar.gz
    ```

2. (Optional) Verify checksum

    ```shell
    curl -fLO https://github.com/gevulotnetwork/gvltctl/releases/download/${VERSION}/gvltctl-${PLATFORM}.tar.gz.sha256
    sha256sum -c gvltctl-${PLATFORM}.tar.gz.sha256
    ```

3. Install the binary

    ```shell
      tar xf gvltctl-${PLATFORM}.tar.gz
      cp gvltctl-${PLATFORM}/gvltctl $HOME/.local/bin
    ```

### Compiling from sources

To compile `gvltclt` crate you will need following dependencies:

- [`protoc`](https://grpc.io/docs/protoc-installation/)
- [`buf`](https://buf.build/docs/installation/)

```shell
cargo install --git https://github.com/gevulotnetwork/gvltctl.git --tag $VERSION
```

## Usage

```plain
$ gvltctl --help
Gevulot Control CLI

Usage: gvltctl <COMMAND>

Commands:
  worker               Commands related to workers
  pin                  Commands related to pins
  task                 Commands related to tasks
  workflow             Commands related to workflows
  keygen               Generate a new key
  compute-key          Compute a key
  send                 Send tokens to a receiver on the Gevulot network
  account-info         Get the balance of the given account
  generate-completion  Generate shell completion scripts
  sudo                 Perform administrative operations with sudo privileges
  build                Build a VM image from a container, rootfs directory, or Containerfile
  help                 Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

## Supported platforms

`gvltctl` is supported on both Linux and MacOS (Windows is not tested, but probably also works).

Building Linux VM (`gvltctl build`) is only supported on Linux right now.
