## Prerequisites

To contribute code changes to this project you will need the following:

* [Rust](https://www.rust-lang.org/tools/install) (minimum version: 1.85, install via `rustup`)
* [Docker](https://docs.docker.com/engine/installation/)

Check your Rust version:
```bash
rustc --version
# rustc 1.85.0 (...)
```

## Checking out the code

```bash
git clone git@github.com:<yourfork>/Watch-Tower-NG.git
cd Watch-Tower-NG
```

## Building and testing

All build commands run from `dev/work/watchtower-rs/`. Load the local sccache config first (optional but recommended):

```bash
# Load local sccache config (Git Bash, one-time per shell)
source dev/runtime/local.env

# Build
cargo build

# Run all tests
cargo test

# Lint (warnings treated as errors)
cargo clippy -- -D warnings

# Run a single test by name
cargo test <testname>
```

## Building the Docker image

```bash
docker build . -f dockerfiles/Dockerfile.dev-self-contained -t wiki-mod/watch-tower-ng
```

## Code style

* `#![forbid(unsafe_code)]` is enforced across the entire crate — no unsafe blocks.
* Warnings are errors: `cargo clippy -- -D warnings` must pass clean.
* 1:1 porting rule: every Go source file maps to exactly one Rust target file.

## License

This project is licensed under the Apache License, Version 2.0. See `LICENSE` and `NOTICE` at the repository root.
