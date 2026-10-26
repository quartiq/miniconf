# `miniconf-mqtt` Python client

Python 3.11+ client and CLI for `miniconf_mqtt` targets.

```sh
python -m pip install -e py/miniconf_mqtt/
```

## Client

Use the async client for schema-aware access:

```python
from miniconf_mqtt.client import Miniconf

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
`connect()` enables TCP_NODELAY for TCP connections; caller-supplied `socket_options` are applied
after this default, allowing an explicit override.

To subscribe before sending commands, use `subscribe()`. Entering the context waits for the
subscription to be ready; it does not request retained replay unless `replay=True`:

```python
async with Miniconf.connect("mqtt", "app/id") as mc:
    async with mc.subscribe() as events:
        await mc.set("/path", 42)
        async for event in events:
            print(event.path, event.value if event.present else "<unavailable>")
```

SET replies acknowledge requests; settings events independently report observed device state.
An event is not correlated with a particular SET. Other readers can cause retained values to repeat.

Use `RawMiniconf` for devices without schema support. Offline schema navigation
and key conversion live in `miniconf_mqtt.schema`; discovery and retained-topic
maintenance live in `miniconf_mqtt.operations`. Method docstrings describe their
contracts.

Notes:

- The client accepts Miniconf MQTT protocol `proto=1`, keeps `/alive` subscribed, and reloads
  schema when `schema_rev` changes. An unsupported protocol invalidates the cache and
  subsequent schema-based operations raise a protocol error. `RawMiniconf` skips this check.
  Rust and Python package versions need not match; incompatible wire changes require a new
  protocol version.
- Exact reads and open watches do not wait for subtree quiescence.
- A finite operation's timeout covers startup, schema loading, subscription, and response waits
  together. Watch timeouts bound setup, not the lifetime of the stream.
  Subscription cleanup has a separate one-second allowance.
- Finite retained subtree snapshots use a quiescence window because MQTT retained replay has no
  end-of-set marker. If the timeout expires before quiescence, snapshots and pruning raise
  `TimeoutError` instead of accepting partial results.
  Snapshots are incremental observations, not atomic reads or guarantees of a single epoch.

## CLI

```sh
miniconf-mqtt --broker mqtt app/id /path
miniconf-mqtt --broker mqtt app/id /path=42
miniconf-mqtt --broker mqtt app/id '/path?'
miniconf-mqtt --broker mqtt app/id '/path!'
miniconf-mqtt --broker mqtt -d 'app/+'
miniconf-mqtt --broker mqtt --raw app/id /path
miniconf-mqtt --broker mqtt --subscribe app/id /path=42
```

Command suffixes:

- `PATH` reads one exact leaf.
- `PATH=VALUE` writes one JSON value and is silent after the device acknowledges it.
  A rejected SET prints the device's error to stderr and exits unsuccessfully, without a traceback.
  `--fire-and-forget` is also silent and does not request a device reply.
- `--subscribe` enters the library's `subscribe()` context before commands, prints settings as
  `PATH=VALUE`, and stays open until Ctrl-C. Empty publications print `PATH=<unavailable>`.
  Updates are independent of SET replies. No initial retained dump is requested.
  This also works with `--raw`, `--fire-and-forget`, or no commands. Errors go to stderr.
- `PATH?` renders schema; `PATH??` prints compact schema NDJSON.
  Indexed nodes show `len=N` (indices start at zero); `{index}` describes
  every child of a homogeneous node once.
- `PATH!` renders retained subtree values; `PATH!!` prints raw `/path=value` lines.
- The root path is empty: use the quoted command `'?'` for root schema. `'/?'` addresses the
  empty-name child below root; `/` is not an alias for root.
- `--raw` disables schema, subtree tracking, `?`, and `!`.
- `--prune PATH` clears stale retained schema/settings below `PATH`.
- `--force-prune` clears all retained topics under the resolved prefix.

Reads, schema queries, and subscriptions print data to stdout. Successful SETs and
prunes are silent. Errors and verbose diagnostics go to stderr.

`--discover` requires exactly one match, with or without commands;
otherwise the error lists candidates on stderr, one prefix per line.
Commands run in order and stop at the first error. Exit status is 0 on success,
1 for an operation failure, and 2 for invalid command-line options.
