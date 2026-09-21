# Miniconf Code Size Benchmark

Compare serial-style get/set using miniconf vs hand-written handler:

- `manual`: manual parser + manual dispatch/get/set.
- `miniconf`: same command protocol, miniconf path lookup on every command, same backend codec.
- `baseline`: parser/loop baseline for size context.

The manual variant is a fair lower bound for the routed get/set workload, not a
feature-equivalent replacement for miniconf. It does not provide schema
iteration, metadata, key transcoding, generic key backends, or generated
reflection. The `schema` column reports the static miniconf schema payload
separately; it is already part of `rodata` and `∑ flash`, not an extra addend.

## Reproduce

Install Rust's `thumbv7m-none-eabi` target, `qemu-system-arm`, and
`arm-none-eabi-size` (usually provided by `binutils-arm-none-eabi`), then run
from the repository root:

```sh
rustup target add thumbv7m-none-eabi
cd miniconf/tests/benchmark
./run.sh
```

The script builds release binaries, checks their workload results in QEMU,
and prints source/compiler provenance with the table. Preserve that output
and this directory's `Cargo.lock` when comparing runs. Use a clean checkout
for a publishable source revision; a `-dirty` suffix means local changes were
included. CI also runs the harness, using its configured nightly toolchain.

The target is Cortex-M3 (`thumbv7m-none-eabi`), run on QEMU's `lm3s6965evb`.
The [manifest](Cargo.toml) selects size optimization (`opt-level = "s"`), LTO,
and one codegen unit. It builds Miniconf with only `derive`; the common codec
is a [small harness-specific Serde backend](src/codec.rs), not JSON or Postcard.
The [workload](src/lib.rs) traverses the same mixed settings tree for both
implementations. This measures neither transport overhead nor hardware latency.

`stack` is the painted-stack high-water mark observed during that program's
execution, including harness work. It is not a worst-case bound for an
application. `∑ ram` adds it to `data + bss`; `∑ flash` is `text + rodata`.
Absent ELF data sections count as zero; absent runtime measurements fail the run.

## Binary size

Example run with program sources at `7f677a3`, Rust 1.98.1 (LLVM 22.1.8),
and no `RUSTFLAGS`. Dependencies are resolved locally; retain `Cargo.lock`
(its hash is printed by the script) for an exact dependency comparison.
Compiler and dependency changes can alter these totals.

Recorded lockfile SHA-256:
`76ac96d05b8fb116ca4d3af8e0f17f50fde01c5339ad921829841d2752ebe93b`.

| variant | text | rodata | schema | stack | data | bss | **∑ ram** | **∑ flash** |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| baseline | 1636 | 2140 | 0 | 92 | 0 | 8 | **100** | **3776** |
| manual | 9572 | 2524 | 0 | 680 | 0 | 8 | **688** | **12096** |
| miniconf | 10036 | 3172 | 1172 | 824 | 0 | 8 | **832** | **13208** |
