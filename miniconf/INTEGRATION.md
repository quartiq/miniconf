# Add a field. Keep the interfaces.

A device gains an averaging control. Where must its name appear?

With Miniconf, start in the Rust type:

```rust
use miniconf::Tree;

#[derive(Clone, Debug, Default, PartialEq, Tree)]
struct Settings {
    enabled: bool,
    averaging: u16,
}
```

`/averaging` is now available to leaf access and schema traversal. A generic
consumer can find it without adding a command, persistence key, or UI declaration.
The application still supplies its meaning, valid range, and hardware effect.

## One tree, two consumers

Stabilizer's [miniconf-settings](https://github.com/quartiq/stabilizer/tree/292f6f3fa15b4a51789d97a08cbd7546ca3f3d06/miniconf-settings)
puts a shell and snapshots above that boundary:

```text
set /averaging 16   -> changes the live tree
get /averaging     -> reads the same leaf
store              -> asks the application to persist the tree
reset              -> selects defaults for next boot; live values stay put
```

The [shell](https://github.com/quartiq/stabilizer/blob/292f6f3fa15b4a51789d97a08cbd7546ca3f3d06/miniconf-settings/src/shell.rs)
resolves paths and calls Miniconf. The
[snapshot layer](https://github.com/quartiq/stabilizer/blob/292f6f3fa15b4a51789d97a08cbd7546ca3f3d06/miniconf-settings/src/snapshot.rs)
walks the tree and encodes leaf records. Both pick up the new field.
USB framing and hardware updates stay with the application.

This crate is currently unpublished. It demonstrates a reusable upper layer;
the [core quickstart](README.md#quick-start) runs on its own.

## Change the consumer

The same approach works for an inspector, a configuration editor, or a protocol:
use `TreeSchema` to discover leaves, `TreeSerialize` to read, and
`TreeDeserialize` to write. Require only the capabilities the consumer needs.

Miniconf exposes the field; it does not invent validation or application policy.
See [validation and application effects](README.md#validation-and-application-effects)
before making changes live.

Try the existing [CLI](examples/cli.rs), [compact-key example](examples/packed.rs),
or [schema generator](examples/trace.rs). Add a field to their shared
[settings type](examples/common.rs) and give it a default. Which parts of the
consumer actually need to change?
