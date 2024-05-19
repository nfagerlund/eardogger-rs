#!/bin/bash

# This is all pretty dumb, but I just need something to speed up deploys during soak testing.

# cross-build for linux x64, requires appropriate toolchain stuffs
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-unknown-linux-gnu-gcc cargo build --release --target=x86_64-unknown-linux-gnu
# tar's -C flag changes to a dir before processing remaining files.
tar -czf eardogger-release.tar.gz README.md VERSION.txt eardogger.example.toml public migrations htaccess-example -C target/x86_64-unknown-linux-gnu/release eardogger-rs
