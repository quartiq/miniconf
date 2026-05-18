# `miniconf-mqtt` Python client

Python 3.11+ client and CLI for MM2 `miniconf_mqtt` targets.

```sh
python -m pip install -e py/
```

## Client

Use the async client for schema-aware access:

```python
from aiomqtt import Client
from miniconf.client import Miniconf
from miniconf.common import MQTTv5

async with Client("mqtt", protocol=MQTTv5) as mqtt:
    async with Miniconf(mqtt, "app/id") as mc:
        schema = await mc.schema()
        await mc.set("/path", 42)

        async with mc.track("/subtree") as tracked:
            value = tracked.cached("/subtree/leaf")
            cached = tracked.snapshot()
```

Core API:

- `schema()` loads and caches the retained MM2 schema.
- `set(path, value, response=True)` publishes one `set/#` request.
- `track(path="")` scopes a retained subtree cache; use `cached()` for one leaf or
  `snapshot()` for values below the tracked root.
- `RawMiniconf` provides exact-path `get()` and `set()` without schema loading.

Schema helpers:

- `schema.path(keys)` normalizes string, `Indices`, `Packed`, or path-part keys.
- `schema.node(keys)` returns a `SchemaNode(path, schema)` with `.kind`, `.node`, and `.edge`.
- `schema.children(keys)` returns direct child nodes.
- `schema.walk(keys="")` iterates a subtree.
- `schema.compact(keys="")` returns compact defs rooted at a subtree.
- `schema.indices(keys)` and `schema.packed(keys)` translate paths to Rust-compatible keys.

Notes:

- The client keeps `/alive` subscribed and reloads tracked settings when `epoch`
  changes; it reloads schema when `schema_rev` changes.
- Schema-aware reads are explicit: open `track()` for the subtree you want, then read its cache.
- Retained `/settings` messages without `auth=""` are ignored as non-authoritative MM2 traffic.
- Retained burst quiescence uses the same rule as the Rust client:
  `100 ms + 3 * measured_subscribe_rtt`, reset on each accepted retained publication.

## CLI

```sh
miniconf --broker mqtt app/id /path
miniconf --broker mqtt app/id /path=42
miniconf --broker mqtt app/id /path?
miniconf --broker mqtt app/id /path!
miniconf --broker mqtt --raw app/id /path
```

Command suffixes:

- `PATH` reads one exact leaf.
- `PATH=VALUE` writes one JSON value with ACK/NACK by default.
- `PATH?` renders schema; `PATH??` prints compact schema NDJSON.
- `PATH!` renders retained subtree values; `PATH!!` prints raw `/path=value` lines.
- `--raw` disables schema, subtree tracking, `?`, and `!`.
- `--prune PATH` clears stale retained schema/settings below `PATH`.
- `--force-prune` clears all retained MM2 topics below the resolved prefix.
