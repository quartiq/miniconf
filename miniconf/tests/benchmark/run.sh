#!/usr/bin/env bash
set -euo pipefail

TARGET_DIR="target/thumbv7m-none-eabi/release"

bins=(
  baseline
  manual
  miniconf
)

cargo build --release --bins

echo "## Binary size"
echo "| variant | text | rodata | data | bss | flash | ram |"
echo "|---|---:|---:|---:|---:|---:|---:|"

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
  echo "| $bin | $text | $rodata | $data | $bss | $flash | $ram |"
done
