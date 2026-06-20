#!/bin/bash

BINFILE=watchtower-rs
if [ -n "$MSYSTEM" ]; then
    BINFILE=watchtower-rs.exe
fi
VERSION=$(git describe --tags)
echo "Building $VERSION..."
WATCHTOWER_VERSION="$VERSION" cargo build --locked --release --bin watchtower-rs
cp "target/release/$BINFILE" "$BINFILE"
