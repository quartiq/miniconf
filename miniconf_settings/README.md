# Miniconf Settings

Settings commands and snapshot persistence for Miniconf trees.
`no_std`, no allocator; the application owns I/O, command dispatch, defaults,
and when changes take effect.

## Try it

```sh
cargo run -p miniconf_settings --example shell
```

```text
> get /output/dac
/output/dac/0: 1024
/output/dac/1: 1024
> set /output/dac/0 2048
Set.
> set /output/dac/0 4096
error: Access/validation failure: DAC value exceeds 12-bit range
> schema /output/dac
/output/dac [homogeneous]
  0..2 [leaf] [sem ty=i16]
```

The example keeps settings in memory; piped commands also work.
Tab completes commands and path segments. Type `/` to enter a subtree.
When completion stops, repeat Tab to list matching names in columns. Branch names
show `/`; large indexed collections show an index range. Type an index to select
it directly. Lists are limited to four rows; narrow the prefix for more.
Use `schema` to describe a whole subtree. Arrow keys edit and recall history,
Ctrl-C cancels, and Ctrl-D exits an empty line.

Command paths start with `/`, are unquoted, and contain no whitespace. Omit the
path for root `get` or `schema`; `set` requires a path.

## Integrate

- `shell::Terminal` drives a Noline editor over async I/O, with completion and
  match listing. Supply editor buffers and your command names; dispatch the
  returned line in your application.
- `shell::Command` parses commands for your dispatcher.
  `write_values` reads a leaf or subtree; `write_schema` describes it.
- `shell::complete` returns a replacement or match context for a command
  line when integrating another editor.
- `snapshot` encodes and restores JSON leaf records.
- `flash::Store` saves snapshots with fallback to the previous snapshot.

Apply assignments with `miniconf::json_core::set` and record successful changes
before replying. The application dispatches persistence and board commands.
Include application commands in the terminal's command list and route
`ParseError::UnknownCommand` to your own parser. Their names complete normally;
their arguments remain application-defined. `complete_path` supplies path
candidates for custom command syntaxes.

For shared settings, combine `NodeIter` with `write_value` to release settings
before awaiting output.

Enable Miniconf's `sem`, `meta-node`, and `meta-edge` features for type information,
descriptions, and units. Inspection and completion describe schema possibilities,
not runtime presence or writability.

## Persistence

Load a store or explicitly erase it before storing or resetting.
Loading tries the newest valid snapshot, then the older one; `load_validated`
also applies application validation. Failure leaves the caller's settings
unchanged when clones have independent mutable state. Setter and validation side
effects are not rolled back.

Unknown paths are ignored; missing leaves keep the caller's defaults.
Incompatible values at known paths reject the snapshot. Applications own migrations.

Persist a writable settings view, excluding diagnostics and read-only leaves.
Flash must hold both snapshots plus space for compaction; two erase pages alone
do not guarantee enough capacity.

`store` saves the current tree. `reset` selects defaults for the next load
without changing current settings. Erasing unusable storage is explicit.

## Features and limits

The default `shell` and `flash` features are independent; disable both for
snapshot records alone. Traversal supports `MAX_DEPTH` levels, printed and
stored paths up to 128 bytes, and snapshot JSON values up to 512 bytes per leaf.

Set `DEFMT_LOG=debug RUST_LOG=debug` for diagnostics in the host example.
