# Miniconf Code Size Benchmark

Compare serial-style get/set using miniconf vs hand-written handler:

- `manual`: manual parser + manual dispatch/get/set.
- `miniconf`: same command protocol, miniconf path lookup on every command, same backend codec.
- `miniconf_dyn`: the same binary rebuilt with `erased-keys`, changing only
  the path cursor to `&mut dyn Keys`.
- `baseline`: parser/loop baseline for size context.

The manual variant implements only the routed get/set workload. Miniconf's path
lookup uses a static schema; discovery and reflection APIs are not exercised here.

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
harness with the stable toolchain.

The target is Cortex-M3 (`thumbv7m-none-eabi`), run on QEMU's `lm3s6965evb`.
The [manifest](Cargo.toml) selects `opt-level = "s"`, LTO, one codegen unit,
and Miniconf's `derive` feature only. Both implementations run the same
[workload](src/lib.rs) through a [custom Serde codec](src/codec.rs).
JSON, Postcard, transports, and hardware latency are outside this measurement.

Before the workload, each implementation checks every write against its requested
value. The workload uses canonical scalar encodings, allowing a byte comparison.
The host regression test also checks that ignoring writes fails validation:

```sh
cargo test --lib --target x86_64-unknown-linux-gnu
```

`stack` is the painted-stack high-water mark observed during that program's
execution, including harness work. It is not a worst-case bound for an
application. `∑ ram` adds it to `data + bss`; `∑ flash` is `text + rodata`.
Absent ELF data sections count as zero; absent runtime measurements fail the run.
`schema` is the static schema payload, already included in `rodata` and `∑ flash`.

## Binary size

Program sources: `a99b827-dirty` with the key-dispatch comparison.
Compiler: Rust 1.98.1 (LLVM 22.1.8).
`RUSTFLAGS` unset. Dependencies are resolved locally; totals can change with
the compiler or dependency versions.

Recorded lockfile SHA-256:
`76ac96d05b8fb116ca4d3af8e0f17f50fde01c5339ad921829841d2752ebe93b`.

| variant | text | rodata | schema | stack | data | bss | **∑ ram** | **∑ flash** |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| baseline | 1636 | 2140 | 0 | 92 | 0 | 8 | **100** | **3776** |
| manual | 8412 | 2388 | 0 | 560 | 0 | 8 | **568** | **10800** |
| miniconf | 8880 | 3036 | 1172 | 728 | 0 | 8 | **736** | **11916** |

Key erasure increases flash by about 6.5% in this single-cursor workload.
This comparison does not exercise duplication across different key representations.
