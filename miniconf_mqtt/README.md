# `miniconf` MQTT Client

This crate exposes a [`miniconf`](https://crates.io/crates/miniconf) tree over MQTT using
[`minimq`](https://crates.io/crates/minimq).

It is built around one long-lived `minimq::Session`: reconnects, keepalive, and MQTT request/reply
routing stay inside the MQTT layer, while `miniconf_mqtt` only adds settings-tree behavior on top.

## Driver Model

`MqttClient` is a manually driven async service.

- Call `poll()` regularly. It drives the MQTT session, handles inbound requests,
  activates the settings service after each connection, and drains queued multipart
  replies and dumps.
- Call `activate()` only if you need the retained `/<prefix>/alive` publish and
  `/<prefix>/settings/#` subscription installed before your first application
  `publish()`. Regular `poll()` calls do this automatically.
- Call `publish()` for application messages on the shared MQTT session. It calls
  `activate()` first.
- Call `dump()` to queue a settings dump. The dump is emitted incrementally by
  subsequent `poll()` calls; `dump()` itself does not publish anything.
- Call `can_publish()` only to check local publish capacity. It is pessimistic
  for local backpressure: `false` means a publish would currently be blocked by
  local session capacity. It is optimistic overall: `true` still does not rule
  out serialization, packet-size, disconnect, or transport failures from the
  actual `publish()`.

## MQTT Contract

The public MQTT behavior is intentionally stable and matches the Python client in
[`py/miniconf-mqtt`](../py/miniconf-mqtt).

Topics:

- settings requests and dumps use `"<prefix>/settings<path>"`
- liveness uses `"<prefix>/alive"`
- with the `introspection` feature, static schema requests use `"<prefix>/schema<path>"`
- with the `introspection` feature, runtime state requests use `"<prefix>/state<path>"`

Request routing:

| Request | Path kind | Response topic | Payload | Result |
| --- | --- | --- | --- | --- |
| `GET` | leaf | set | empty | one reply payload with the serialized leaf value |
| `LIST` | internal | set | empty | multipart replies on the response topic |
| `DUMP` | leaf or internal | not set | empty | publishes leaf values under `"<prefix>/settings/..."` |
| `SET` | leaf | optional | non-empty | updates the leaf and replies `Ok` if a response topic is present |
| invalid `SET` | internal | optional | non-empty | request is rejected |

More precisely:

- An empty payload means a read-like request.
- A non-empty payload means `SET`.
- An internal-node request with a response topic is `LIST`.
- An internal-node request without a response topic is `DUMP`.
- Unknown topics under other prefixes are ignored.
- Overlong leaf paths are rejected rather than truncated.
- Path-resolution errors report the actual consumed depth.
- `schema` and `state` requests are read-only and require a response topic.

## Response Semantics

One-shot replies and multipart `LIST` replies use the MQTT v5 `ResponseTopic` and
`CorrelationData` properties.

User property `code` distinguishes reply state:

- `Ok`: terminal success
- `Continue`: more multipart `LIST` payloads follow
- `Error`: request rejected or operation failed

`DUMP` does not use the response topic. It publishes current settings values directly into the
settings namespace and does not wait for completion.

## Introspection

Enable the `introspection` feature to expose compact schema and runtime-state queries.

- `schema` returns one node descriptor for the addressed path.
- `state` returns runtime accessibility information for the addressed path.
- Both use normal MQTT v5 request/reply on the response topic.

`schema` replies mirror `miniconf::Schema`:

- leaf: `{"attrs":...,"sem":...}` or `{}`
- named internal: `{"internal":{"kind":"named","children":[{"name":"child","attrs":...},...]},...}`
- numbered internal: `{"internal":{"kind":"numbered","children":[{"attrs":...},...]},...}`
- homogeneous internal: `{"internal":{"kind":"homogeneous","child":{"attrs":...},"len":N},...}`

Node attrs and sem are attached to the addressed node. Child-edge attrs are kept on the parent
internal node, mirroring `Schema`/`Internal`. Current structured semantics include:

- `oneof` for mutually exclusive named internals
- `maybe_absent` for nodes that may be absent at runtime

This is the intended channel for downstream hints such as:

- `#[tree(meta(enum))]` on enums, which yields `{"sem":{"oneof":true}}`
- `Option<T>`, which yields `{"sem":{"maybe_absent":true}}`
- `#[tree(meta(switches = "mode"))]` on selector fields, exposed on that child edge

`state` replies are compact JSON objects:

- present subtree: `{"state":"present"}`
- absent subtree: `{"state":"absent"}`
- unknown/inaccessible subtree: `{"state":"unknown"}`
- enum-like subtree with one active child: `{"state":"present","active":"Variant"}`

## Notes

- `LIST` emits leaf paths below the requested internal node.
- Multipart operations temporarily monopolize the interface; concurrent requests may be rejected
  with `Error`.
- The example client triggers the initial dump explicitly. That behavior is part of the example and
  Python smoke-test workflow, not an implicit protocol bootstrap rule.
