# Miniconf Code Size Benchmark

Compare serial-style get/set using miniconf vs hand-written handler:

- `manual`: manual parser + manual dispatch/get/set.
- `miniconf`: same command protocol, miniconf path lookup on every command, same backend codec.
- `baseline`: parser/loop baseline for size context.

## Binary size

| variant | text | rodata | data | bss | flash | ram |
|---|---:|---:|---:|---:|---:|---:|
| baseline | 512 | 1356 | 0 | 8 | 1868 | 8 |
| manual | 8160 | 1900 | 0 | 8 | 10060 | 8 |
| miniconf | 7904 | 2292 | 0 | 8 | 10196 | 8 |
