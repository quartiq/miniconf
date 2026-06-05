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

## Binary size
| variant | text | rodata | schema | stack | data | bss | **∑ ram** | **∑ flash** |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| baseline | 1636 | 2140 | 0 | 92 | 0 | 8 | **100** | **3776** |
| manual | 9668 | 2500 | 0 | 680 | 0 | 8 | **688** | **12168** |
| miniconf | 10108 | 3152 | 1172 | 824 | 0 | 8 | **832** | **13260** |
