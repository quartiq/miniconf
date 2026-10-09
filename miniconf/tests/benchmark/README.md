# Settings command interface: code size

What does Miniconf cost in a small command interface compared with handwritten
field access?

Both implementations expose the unchanged [example settings](../../examples/common.rs):
`/path;` reads and `/path=JSON;` writes. Tree's optional `help` feature adds
`help [/path];`, describing a node and its immediate children.
Help includes node type names, field documentation,
units and limits, and semantic scalar types and optionality. Metadata stays
on the node where it is declared; optionality does not report runtime availability.
The comparison uses canonical absolute, case-sensitive paths. Replies are values, `OK`, or errors;
execution continues after an error. Quoted semicolons are not supported.
Error precedence for commands with multiple faults is not compared.

- **Manual:** direct path matching, typed JSON get/set, and explicit access checks.
- **Tree:** Miniconf's JSON get/set functions and the fixture's access rules;
  with `help`, structure, node/edge metadata, and semantics come from `Settings::SCHEMA`.

The command parser is shared. One persistent settings instance serves a
[command transcript](src/commands.txt), followed by a sensor update and loss of
calibration. With `help`, a [help transcript](src/help-commands.txt) follows.
Host tests and QEMU replies are checked against the same
[expected output](src/replies.txt); the firmware contains no expected replies
or validation comparisons. [Help replies](src/help-replies.txt) are checked separately.

## Results

Rust 1.99.0 (LLVM 23.1.1), Cortex-M3 (`thumbv7m-none-eabi`),
`opt-level = "s"`, LTO, one codegen unit, Ryu `small`. Each variant is built separately.
Code and constants are `.text + .rodata`, including the transcript driver but
excluding the 1,024-byte vector table. Observed stack is the painted high-water
mark during the transcript, including semihosting, not a worst-case bound.
All variants have zero `.data` and `.bss`.

| Implementation | Code + constants (bytes) | Observed stack (bytes) |
|---|---:|---:|
| Manual | 15024 | 424 |
| Tree | 15968 | 504 |
| Tree + help | 19848 | 784 |

For this interface, Tree replaces handwritten field dispatch and access checks
for 944 extra bytes of code and constants and 80 extra bytes of observed stack.
Adding schema-derived help costs another 3,880 bytes and 280 bytes of observed
stack, including metadata and the help transcript. Metadata features are enabled
only with `help`.

Run `./run.sh` with QEMU, ARM binutils, and the Rust `thumbv7m-none-eabi` target installed.
