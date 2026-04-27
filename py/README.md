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
        await mc.watch("/path")
        value = await mc.cached(schema.path(Packed(0b1_1_100)))
        await mc.set("/path", 42)
        tree = await mc.snapshot("/subtree")
    finally:
        await mc.close()
```

Core operations:

- `schema()` loads and caches the retained schema object
- `watch(path="/")` subscribes to retained authoritative settings below one subtree
- `unwatch(path="/")` undoes one watch reference
- `cached(path)` reads one cached retained value without changing subscriptions
- `snapshot(path="")` returns cached retained authoritative values below one subtree
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

- `cached()` is the no-side-effect read for already watched subtrees
- successful `set()` returns `None`; the applied value is published on `settings/#`
- explicit ACK/NACK replies on `response` are metadata-only
- the long-lived client keeps `/alive` subscribed and invalidates cached schema/settings when
  `epoch` or `schema_rev` changes
- retained `settings/#` without `rev` is ignored everywhere as non-authoritative MM2 traffic
- retained settings watching is long-lived and uses a burst-settle timeout heuristic
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

- `PATH` reads one cached retained value from `settings/<path>`
- `PATH=VALUE` writes one `set/<path>` value with explicit ACK/NACK by default
- `PATH?` prints a human-readable schema tree below `PATH`
- `PATH??` prints compact schema defs below `PATH` as NDJSON
- `PATH!` prints a human-readable value tree below `PATH`
- `PATH!!` prints retained authoritative settings below `PATH` as `/path=value`
- `-n/--fire-and-forget` disables the explicit reply and only sends the `set/#` request
- `-d/--discover` resolves a unique device prefix through `alive`
- `--raw` switches to exact-path `GET`/`SET` only: no schema, no tracked retained cache, no
  `?`, `??`, `!`, or `!!`
- `--prune PATH` clears stale retained schema pages and stale retained settings below `PATH`
- `--force-prune` clears all retained MM2 topics below the resolved prefix

## Related docs

- Rust crate: [../../miniconf_mqtt/README.md](../../miniconf_mqtt/README.md)
- Example smoke/integration test: [../test.py](../test.py)
