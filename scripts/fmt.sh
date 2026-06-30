#!/usr/bin/env bash
set -euo pipefail

if [ -f "$HOME/export-esp.sh" ]; then
  source "$HOME/export-esp.sh"
fi

cargo +esp clippy \
  -p datex-embedded \
  --fix \
  --features target_esp32s3 \
  --target xtensa-esp32s3-none-elf \
  --allow-dirty

cargo fmt --all
git commit -a -m "fmt"