# `miniconf` MQTT Client

This crate exposes a [`miniconf`](https://crates.io/crates/miniconf) tree over MQTT using
[`minimq`](https://crates.io/crates/minimq).

It is built around one long-lived `minimq::Session`: reconnects, keepalive, and MQTT request/reply
routing stay inside the MQTT layer, while `miniconf_mqtt` only adds settings-tree behavior on top.

## MQTT Contract

The public MQTT behavior is intentionally stable and matches the Python client in
[`py/miniconf-mqtt`](../py/miniconf-mqtt).

Topics:

- settings requests and dumps use `"<prefix>/settings<path>"`
- liveness uses `"<prefix>/alive"`

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

## Response Semantics

One-shot replies and multipart `LIST` replies use the MQTT v5 `ResponseTopic` and
`CorrelationData` properties.

User property `code` distinguishes reply state:

- `Ok`: terminal success
- `Continue`: more multipart `LIST` payloads follow
- `Error`: request rejected or operation failed

`DUMP` does not use the response topic. It publishes current settings values directly into the
settings namespace and does not wait for completion.

## Notes

- `LIST` emits leaf paths below the requested internal node.
- Multipart operations temporarily monopolize the interface; concurrent requests may be rejected
  with `Error`.
- The example client triggers the initial dump explicitly. That behavior is part of the example and
  Python smoke-test workflow, not an implicit protocol bootstrap rule.
