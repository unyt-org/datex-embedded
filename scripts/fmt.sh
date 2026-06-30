#!/usr/bin/env bash
set -euo pipefail
cargo clippy -p datex-embedded --fix --features target_esp32s3 --target xtensa-esp32s3-none-elf --allow-dirty
cargo clippy --workspace --exclude datex-embedded --fix --allow-dirty --target xtensa-esp32s3-none-elf --allow-dirty
cargo fmt --all
git commit -a -m "fmt"