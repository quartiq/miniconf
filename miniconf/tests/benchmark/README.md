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

Keep the output and `Cargo.lock`: the report includes source revision, lockfile
hash, and compiler. `-dirty` marks local source changes. CI runs the same
harness with its configured nightly toolchain.

The target is Cortex-M3 (`thumbv7m-none-eabi`), run on QEMU's `lm3s6965evb`.
The [manifest](Cargo.toml) selects `opt-level = "s"`, LTO, one codegen unit,
and Miniconf's `derive` feature only. Both implementations run the same
[workload](src/lib.rs) through a [custom Serde codec](src/codec.rs).
JSON, Postcard, transports, and hardware latency are outside this measurement.

`stack` is the painted-stack high-water mark observed during that program's
execution, including harness work. It is not a worst-case bound for an
application. `∑ ram` adds it to `data + bss`; `∑ flash` is `text + rodata`.
Absent ELF data sections count as zero; absent runtime measurements fail the run.

## Binary size

Program sources: `7f677a3`. Compiler: Rust 1.98.1 (LLVM 22.1.8).
`RUSTFLAGS` unset. Dependencies are resolved locally; totals can change with
the compiler or dependency versions.

Recorded lockfile SHA-256:
`76ac96d05b8fb116ca4d3af8e0f17f50fde01c5339ad921829841d2752ebe93b`.

| variant | text | rodata | schema | stack | data | bss | **∑ ram** | **∑ flash** |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| baseline | 1636 | 2140 | 0 | 92 | 0 | 8 | **100** | **3776** |
| manual | 9572 | 2524 | 0 | 680 | 0 | 8 | **688** | **12096** |
| miniconf | 10036 | 3172 | 1172 | 824 | 0 | 8 | **832** | **13208** |
