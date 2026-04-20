# Miniconf Code Size Benchmark

Compare serial-style get/set using miniconf vs hand-written handler:

- `manual`: manual parser + manual dispatch/get/set.
- `miniconf`: same command protocol, miniconf path lookup on every command, same backend codec.
- `baseline`: parser/loop baseline for size context.

## Binary size
| variant | text | rodata | schema | stack | data | bss | **∑ ram** | **∑ flash** |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| baseline | 1636 | 1504 | 0 | 92 | 0 | 8 | **100** | **3140** |
| manual | 8140 | 1872 | 0 | 608 | 0 | 8 | **616** | **10012** |
| miniconf | 7500 | 2228 | 860 | 736 | 0 | 8 | **744** | **9728** |
