#!/usr/bin/env bash
set -euo pipefail
cargo clippy -p datex-embedded --fix --features target_esp32s3
cargo clippy --workspace --exclude datex-embedded --fix --allow-dirty
cargo fmt --all
git commit -a -m "fmt"