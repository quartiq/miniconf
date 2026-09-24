#!/usr/bin/env bash
set -euo pipefail

elf="target/thumbv7m-none-eabi/release/benchmark"

if [ ! -f Cargo.lock ]; then
  cargo generate-lockfile
fi

echo '## Environment'
echo '```text'
git describe --always --dirty
sha256sum Cargo.lock
rustc -Vv
cargo -V
printf 'RUSTFLAGS=%s\n' "${RUSTFLAGS:-}"
echo '```'
echo "## Binary size"
echo "Sizes in bytes; stack is the observed high-water mark, including semihosting."
echo
echo "| variant | text | rodata | observed stack | data | bss | **data + bss + observed stack** | **text + rodata** |"
echo "|---|---:|---:|---:|---:|---:|---:|---:|"

for variant in manual tree manual+help tree+help; do
  feature_list="${variant/manual/}"
  feature_list="${feature_list/+/ }"
  features=(--features "$feature_list")
  expected="$(cat src/replies.txt)"
  if [[ "$variant" = *+help ]]; then
    expected+=$'\n'"$(cat src/help-replies.txt)"
  fi
  cargo build --locked --release --bin benchmark "${features[@]}"
  size_out="$(arm-none-eabi-size -A "$elf")"
  text="$(printf '%s\n' "$size_out" | awk '$1==".text"{print $2}')"
  rodata="$(printf '%s\n' "$size_out" | awk '$1==".rodata"{print $2}')"
  data="$(printf '%s\n' "$size_out" | awk '$1==".data"{print $2}')"
  bss="$(printf '%s\n' "$size_out" | awk '$1==".bss"{print $2}')"
  text="${text:-0}"
  rodata="${rodata:-0}"
  data="${data:-0}"
  bss="${bss:-0}"
  if ! run_out="$(cargo run --locked --quiet --release --bin benchmark "${features[@]}" 2>&1)"; then
    printf '%s\n' "$run_out" >&2
    echo "benchmark validation failed for $variant" >&2
    exit 1
  fi
  replies="$(printf '%s\n' "$run_out" | sed -n '/^BEGIN$/,/^END$/{ /^BEGIN$/d; /^END$/d; p; }')"
  if [ "$replies" != "$expected" ]; then
    printf '%s\n' "$run_out" >&2
    echo "unexpected replies for $variant" >&2
    exit 1
  fi
  stack="$(printf '%s\n' "$run_out" | sed -n 's/^RESULT stack_peak=//p' | tail -n1)"
  if ! [[ "$stack" =~ ^0x[0-9a-f]{8}$ ]]; then
    printf '%s\n' "$run_out" >&2
    echo "missing or invalid stack measurement for $variant" >&2
    exit 1
  fi
  stack=$((stack))
  flash=$((text + rodata))
  ram=$((data + bss + stack))
  echo "| $variant | $text | $rodata | $stack | $data | $bss | **$ram** | **$flash** |"
done
