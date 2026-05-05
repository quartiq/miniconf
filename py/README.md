# `miniconf-mqtt` Python client

Python package for interacting with MM2 `miniconf_mqtt` targets.

Requires Python 3.11 or newer.

## Installation

From this repo:

```sh
python -m pip install -e py/
```

Directly from Git:

```sh
python -m pip install \
  git+https://github.com/quartiq/miniconf#subdirectory=py
```

## Async client

The preferred tracked surface is async-first in [miniconf/async_.py](miniconf/async_.py).

```python
from aiomqtt import Client
from miniconf.async_ import MiniconfClient
from miniconf.schema import Packed
from miniconf.common import MQTTv5

async with Client("mqtt", protocol=MQTTv5) as mqtt:
    mc = MiniconfClient(mqtt, "app/id")
    try:
        schema = await mc.schema()
        value = await mc.get(schema.path(Packed(0b1_1_100)))
        await mc.set("/path", 42)
        async with mc.track("/subtree") as tracked:
            tree = tracked.snapshot()
    finally:
        await mc.close()
```

Core operations:

- `schema()` loads and caches the retained schema object
- `get(path)` reads one exact leaf; tracked coverage is reused when available
- `track(path="/")` returns an async context manager for one tracked retained subtree
- `TrackedSubtree.cached(path="")` reads one cached tracked leaf
- `TrackedSubtree.snapshot(path="")` returns cached retained authoritative values below one tracked subtree
- `set(path, value, response=True)` sends one explicit `set/#` request

Raw exact-path operations:

- `RawMiniconfClient(client, prefix)` skips schema loading and tracked retained-state caching
- `get(path)` reads one exact retained authoritative leaf from `settings/<path>`
- `set(path, value, response=True)` sends one exact `set/<path>` request without schema lookup

Schema operations:

- `schema.compact(keys="")` returns rooted compact schema defs below one subtree
- `schema.record(keys)` returns one exact record
- `schema.node(keys)` returns one typed schema node view
- `schema.contains(keys)` checks whether one key exists
- `schema.paths(keys="")` returns subtree paths
- `schema.children(keys)` returns direct child node views
- `schema.parent(keys)` returns the direct parent node view
- `schema.siblings(keys)` returns sibling node views
- `schema.ty(keys)` returns the `schema` description for one node
- `schema.node_meta(keys)` returns node metadata
- `schema.edge_meta(keys)` returns parent-child edge metadata
- `schema.kind(keys)` returns `"leaf"` or the internal-node kind
- `schema.walk(keys="")` iterates subtree node views
- `schema.path(keys)` normalizes `path`, `indices`, or `packed` keys to one MM2 path
- `schema.indices(keys)` returns hierarchical child indices
- `schema.packed(keys)` returns Rust-compatible LSB `Packed`

Important:

- `get()` is the exact leaf read API
- `track()` is explicit and scoped; cached subtree state exists only inside that scope
- one `MiniconfClient` tracks at most one retained settings subtree at a time
- successful `set()` returns `None`; the applied value is published on `settings/#`
- explicit ACK/NACK replies on `response` are metadata-only
- the long-lived client keeps `/alive` subscribed and invalidates cached schema/settings when
  `epoch` or `schema_rev` changes
- retained `settings/#` without `rev` is ignored everywhere as non-authoritative MM2 traffic
- tracked subtree entry waits for retained-burst quiescence with a timeout heuristic
- the raw client does not keep `alive`, schema, or retained subtree watches; it only does exact
  `GET`/`SET`
- the CLI frontend lives in [miniconf/cli.py](miniconf/cli.py); the client module stays library-focused

## CLI

The installed `miniconf` command uses the async client.

```sh
miniconf --broker mqtt app/id /path
miniconf --broker mqtt --raw app/id /path
miniconf --broker mqtt app/id /path=42
miniconf --broker mqtt -n app/id /path=42
miniconf --broker mqtt app/id /path?
miniconf --broker mqtt app/id /path??
miniconf --broker mqtt app/id /path!
miniconf --broker mqtt app/id /path!!
```

CLI behavior:

- `PATH` reads one exact leaf from `settings/<path>`
- `PATH=VALUE` writes one `set/<path>` value with explicit ACK/NACK by default
- `PATH?` prints a human-readable schema tree below `PATH`
- `PATH??` prints compact schema defs below `PATH` as NDJSON
- `PATH!` prints a human-readable value tree below `PATH`
- `PATH!!` prints retained authoritative settings below `PATH` as `/path=value`
- paths are either empty or start with `/`
- CLI paths that do not start with `/` are interpreted relative to the current base
- absolute subtree commands (`PATH?`, `PATH??`, `PATH!`, `PATH!!`) set the base to `PATH`
- absolute exact leaf reads and SETs set the base to `PATH`'s parent, so sibling leaves can be
  addressed without repeating their common prefix
- `-n/--fire-and-forget` disables the explicit reply and only sends the `set/#` request
- `-d/--discover` resolves a unique device prefix through `alive`
- `--raw` switches to exact-path `GET`/`SET` only: no schema, no tracked retained cache, no
  `?`, `??`, `!`, or `!!`
- `--prune PATH` clears stale retained schema pages and stale retained settings below `PATH`
- `--force-prune` clears all retained MM2 topics below the resolved prefix

## Related docs

- Rust crate: [../../miniconf_mqtt/README.md](../../miniconf_mqtt/README.md)
- Example smoke/integration test: [../test.py](../test.py)
