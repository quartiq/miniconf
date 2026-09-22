# `miniconf-mqtt` Python client

Python 3.11+ client and CLI for `miniconf_mqtt` targets.

```sh
python -m pip install -e py/
```

## Client

Use the async client for schema-aware access:

```python
from miniconf.client import Miniconf

async with Miniconf.connect("mqtt", "app/id") as mc:
    schema = await mc.schema()
    value = await mc.get("/path")
    snapshot = await mc.snapshot("/subtree")
    await mc.set("/path", 42)

    async for event in mc.watch("/subtree"):
        print(event.path, event.value if event.present else "<deleted>")
```

Each context is one session. Connection loss raises `aiomqtt.MqttError` in pending operations and watches;
open a new context to reconnect. When supplying an existing aiomqtt client, enter
`async with Miniconf(client, prefix)` and give it exclusive use of the client's message stream.

Core API:

- `schema()` loads and caches the retained schema.
- `get(path)` reads one schema-validated retained leaf without opening a subtree cache.
- `set(path, value, response=True)` publishes one `set/#` request.
  It waits for the broker's acknowledgement; `response=True` also waits for the device's reply.
- `snapshot(path="")` reads a finite retained subtree snapshot.
- `watch(path="")` streams authoritative retained settings publications below a subtree without
  waiting for quiescence. Events distinguish JSON `null` from retained deletes through `.present`.
  Opening another reader can replay retained values to an existing watcher.
- `RawMiniconf` provides exact-path `get()`, `set()`, `snapshot()`, and `watch()` without schema
  loading.

Schema helpers:

- `schema.path(keys)` normalizes string, `Indices`, `Packed`, or path-part keys.
- `schema.node(keys)` returns a `SchemaNode(path, schema)` with `.kind`, `.node`, and `.edge`.
- `schema.children(keys)` returns direct child nodes.
- `schema.walk(keys="")` iterates a subtree.
- `schema.compact(keys="")` returns compact defs rooted at a subtree.
- `schema.indices(keys)` and `schema.packed(keys)` translate paths to Rust-compatible keys.

Notes:

- The client accepts Miniconf MQTT protocol `proto=1`, keeps `/alive` subscribed, and reloads
  schema when `schema_rev` changes. An unsupported protocol invalidates the cache and
  subsequent schema-based operations raise a protocol error. `RawMiniconf` skips this check.
  Rust and Python package versions need not match; incompatible wire changes require a new
  protocol version.
- Exact reads and open watches do not wait for subtree quiescence.
- A finite operation's timeout covers startup, schema loading, subscription, and response waits
  together. Watch timeouts bound setup and later schema reloads, not the lifetime of the stream.
  Subscription cleanup has a separate one-second allowance.
- Finite retained subtree snapshots use a quiescence window because MQTT retained replay has no
  end-of-set marker. If the timeout expires before quiescence, snapshots and pruning raise
  `TimeoutError` instead of accepting partial results.
- Retained `/settings` messages without `auth=""` are ignored as non-authoritative settings
  traffic.
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
- The root path is empty: use the quoted command `'?'` for root schema. `'/?'` addresses the
  empty-name child below root; `/` is not an alias for root.
- `--raw` disables schema, subtree tracking, `?`, and `!`.
- `--prune PATH` clears stale retained schema/settings below `PATH`.
- `--force-prune` clears all retained topics under the resolved prefix.
