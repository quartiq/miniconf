# Miniconf Code Size Benchmark

Compare serial-style get/set using miniconf vs hand-written handler:

- `manual`: manual parser + manual dispatch/get/set.
- `miniconf`: same command protocol, miniconf path lookup on every command, same backend codec.
- `baseline`: parser/loop baseline for size context.

## Binary size
| variant | text | rodata | data | bss | flash | ram |
|---|---:|---:|---:|---:|---:|---:|
| baseline | 300 | 1272 | 0 | 0 | 1572 | 0 |
| manual | 8068 | 1852 | 0 | 8 | 9920 | 8 |
| miniconf | 7428 | 2708 | 0 | 8 | 10136 | 8 |
