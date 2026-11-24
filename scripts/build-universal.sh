#!/bin/sh
set -eu

mkdir -p dist

rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --locked --target aarch64-apple-darwin
cargo build --release --locked --target x86_64-apple-darwin

lipo -create \
  target/aarch64-apple-darwin/release/rtop \
  target/x86_64-apple-darwin/release/rtop \
  -output dist/rtop

codesign --verify --strict dist/rtop
