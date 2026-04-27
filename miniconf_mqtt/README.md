# `miniconf_mqtt`

`miniconf_mqtt` exposes a [`miniconf`](../miniconf/README.md) tree over MQTT using
[`minimq`](../../minimq/README.md).

The current protocol is MM2:

- retained `/<prefix>/alive` publishes a compact device manifest
- retained `/<prefix>/schema/<n>` publishes paged compact schemata
- retained `/<prefix>/settings/<path>` publishes authoritative leaf values
- `/<prefix>/set/<path>` accepts explicit leaf mutation requests
- `/<prefix>/response` carries metadata-only ACK/NACK replies when requested

## Quick start

See the runnable example in [examples/miniconf.rs](examples/miniconf.rs).
It accepts optional `--broker`, `--prefix`, and `--client-id` arguments.

`MqttClient` is a manually driven async service:

- call `connect(io, settings)` to establish or resume the shared MQTT/MM2 session
- call `poll()` regularly after `connect(io, settings)` to drive bounded MQTT/MM2 progress
- pass a callback to `poll()` for non-MM2 inbound publishes
- handle your own application traffic through the shared session via `publish()`,
  `subscribe()`, and `unsubscribe()`
- call `publish_by_key()` for explicit app-driven retained leaf publication
- call `publish_all(settings)` for an explicit full retained republish after structural or bulk changes
- match on the returned `Event`
  - `Other`: one non-MM2 inbound publish was delivered to the callback

`connect(io, settings)` returns:

- `Connected`: fresh broker session, MM2 request subscriptions were established and the retained
  MM2 schema/settings mirror was republished
- `Reconnected`: broker resumed the existing session and MM2 retained `alive` was republished

`poll()` no longer hides connect/reconnect or background retained sync. Steady-state MM2 progress
is one connected-session poll step plus immediate request handling.

## Manifest

The retained `alive` payload is JSON:

```json
{"epoch":1,"schema_rev":12345678,"pages":7}
```

- `epoch` identifies the current authoritative publication epoch
- `schema_rev` identifies the current schema page generation
- `pages` is the number of retained schema pages

`epoch` changes whenever the device starts a fresh retained MM2 publication cycle. Clients should
invalidate cached retained settings when `epoch` changes, but may reuse a parsed schema if
`schema_rev` is unchanged.

The retained will clears `alive` on disconnect so discovery stays live-device oriented even if
stale retained schema/settings still exist on the broker.

## Schema pages

Each retained `schema/<n>` payload is a UTF-8 text page containing newline-delimited compact
schema definitions.

Each definition looks like:

```json
{"i":{"k":"n","c":{"A":1,"B":2}}}
```

Fields:

- definition ids are implicit: the concatenated line order across pages `0..pages-1` is the
  definition index, and the root definition is the last emitted record
- `m`: metadata for the node or child edge when present
- `s`: structured Miniconf semantics when present in the linked `miniconf` schema
- `i`: internal-node shape when present
- `i.k`: internal kind: `n` named, `d` numbered, `h` homogeneous
- `i.c`: child descriptors
  - named children use object keys for the child names
  - numbered children use an array
  - homogeneous children use one child descriptor plus `i.l`
- child descriptors are either a bare integer ref or `{ "r": <ref>, "m": ... }` when child-edge
  metadata is present

Clients assemble pages `0..pages-1` for the current `schema_rev`.
The revision is FNV-1a over the exact retained schema page payload bytes in page order.

## Settings mirror and `set/#`

Authoritative retained `settings/<path>` publications carry MQTT v5 user property `rev=<u32>`.

- `rev` is monotonic within one `epoch`
- clients should scope `rev` to the current `alive` epoch
- in compatibility mode, `settings/<path>` without `rev` is provisional client traffic, not
  authoritative state
- long-lived clients should keep `alive` subscribed and invalidate cached settings when `epoch`
  or `schema_rev` changes
- clients should ignore `settings/<path>` without `rev`; broad cleanup of such topics is a
  separate maintenance task, not part of authoritative state handling

`set/<path>` accepts one JSON value for one leaf.

- success republishes authoritative retained `settings/<path>`
- if `Response Topic` is present, success also emits an `Ok` reply on `response`
- failure emits only an `Error` reply on `response`
- explicit replies are metadata-only; the applied value is always learned from `settings/#`

## Compatibility mode

Optional feature `compat-settings-ingress` also accepts client writes on `settings/#` for migration
from schema-unaware tools such as MQTT Explorer.

This is degraded:

- raw client writes on `settings/#` are provisional requests
- only device-origin publications carrying `rev` are authoritative
- provisional retained `settings/#` traffic can persist on the broker until the device has
  completed recovery and republished the authoritative mirror
- on startup, the device waits for retained `settings/#` ingress to go quiet before publishing its
  own retained settings mirror
- valid provisional writes update the in-memory settings immediately; invalid ones are answered by
  republishing the current authoritative value once recovery has completed
- long-lived clients should still ignore `settings/#` without `rev`; compatibility mode is for
  ingress from legacy tools, not for authoritative state tracking

## Limitations

- MM2 is small and opinionated. It assumes one authoritative device publisher per MQTT prefix.
- Publication is incremental, not atomic. Clients must treat retained `alive` as the authority for `epoch` and `schema_rev`, and ignore unversioned `settings/#`.
- Only the paged schema wire format is Miniconf-specific. `alive`, `set/#`, and retained `settings/#` stay ordinary JSON plus MQTT metadata.
