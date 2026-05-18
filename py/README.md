# `miniconf-mqtt` Python client

Python 3.11+ client and CLI for MM2 `miniconf_mqtt` targets.

```sh
python -m pip install -e py/
```

## Client

Use the async client for schema-aware access:

```python
from aiomqtt import Client
from miniconf.client import MiniconfClient
from miniconf.common import MQTTv5

async with Client("mqtt", protocol=MQTTv5) as mqtt:
    async with MiniconfClient(mqtt, "app/id") as mc:
        schema = await mc.schema()
        value = await mc.get("/path")
        await mc.set("/path", value + 1)

        async with mc.track("/subtree") as tracked:
            cached = tracked.snapshot()
```

Core API:

- `schema()` loads and caches the retained MM2 schema.
- `get(path)` reads one exact leaf from retained authoritative `/settings`.
- `set(path, value, response=True)` publishes one `set/#` request.
- `track(path="")` scopes a retained subtree cache; use `cached()` for one leaf or
  `snapshot()` for values below the tracked root.
- `RawMiniconfClient` provides exact-path `get()` and `set()` without schema loading.

Schema helpers:

- `schema.path(keys)` normalizes string, `Indices`, `Packed`, or path-part keys.
- `schema.node(keys)` returns a `SchemaNode(path, schema)` with `.kind`, `.node`, and `.edge`.
- `schema.children(keys)` returns direct child nodes.
- `schema.walk(keys="")` iterates a subtree.
- `schema.compact(keys="")` returns compact defs rooted at a subtree.
- `schema.indices(keys)` and `schema.packed(keys)` translate paths to Rust-compatible keys.

Notes:

- The client keeps `/alive` subscribed and invalidates schema/settings caches when `epoch`
  or `schema_rev` changes.
- Retained `/settings` messages without `rev` are ignored as non-authoritative MM2 traffic.
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
