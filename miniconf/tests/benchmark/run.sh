#!/usr/bin/env bash
set -euo pipefail

TARGET_DIR="target/thumbv7m-none-eabi/release"

bins=(
  baseline
  manual
  miniconf
)

cargo build --release --bins
schema_out="$(cargo run --quiet --release --bin schema_size 2>&1)"
schema_bytes="$(printf '%s\n' "$schema_out" | sed -n 's/^RESULT schema_bytes=//p' | tail -n1)"
schema_bytes="${schema_bytes:-0}"

echo "## Binary size"
echo "| variant | text | rodata | data | bss | flash | ram | schema |"
echo "|---|---:|---:|---:|---:|---:|---:|---:|"

for bin in "${bins[@]}"; do
  elf="$TARGET_DIR/$bin"
  size_out="$(arm-none-eabi-size -A "$elf")"
  text="$(printf '%s\n' "$size_out" | awk '$1==".text"{print $2}')"
  rodata="$(printf '%s\n' "$size_out" | awk '$1==".rodata"{print $2}')"
  data="$(printf '%s\n' "$size_out" | awk '$1==".data"{print $2}')"
  bss="$(printf '%s\n' "$size_out" | awk '$1==".bss"{print $2}')"
  text="${text:-0}"
  rodata="${rodata:-0}"
  data="${data:-0}"
  bss="${bss:-0}"
  flash=$((text + rodata))
  ram=$((data + bss))
  schema=0
  if [ "$bin" = "miniconf" ]; then
    schema="$schema_bytes"
  fi
  echo "| $bin | $text | $rodata | $data | $bss | $flash | $ram | $schema |"
done
