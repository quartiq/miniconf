# `miniconf_mqtt`

`miniconf_mqtt` exposes a [`miniconf`](../miniconf/README.md) tree over MQTT using
[`minimq`](../../minimq/README.md).

The protocol is MM2:

- retained `/<prefix>/alive` publishes a compact device manifest
- retained `/<prefix>/schema/<n>` publishes paged compact schemata
- retained `/<prefix>/settings/<path>` publishes authoritative leaf values
- `/<prefix>/set/<path>` accepts explicit leaf mutation requests
- `/<prefix>/response` carries metadata-only ACK/NACK replies when requested

## Quick start

See the runnable example in [examples/miniconf.rs](examples/miniconf.rs).

`minimq::Session` is caller-owned.

For simple services, `miniconf_mqtt` provides two complete unbounded helpers on top:

- `mm2.startup(&mut session, &settings, connect_event)`
- `mm2.serve(&mut session, &mut settings, on_unhandled)`

They are the easiest way to serve MM2 when you do not need stepwise control, bounded queued
follow-up, or exact control over unrelated inbound traffic during MM2 work.
`on_unhandled` is synchronous and called at most once. For async application handling, copy or
extract the needed data there and await after `serve()` returns `Event::Unhandled`.

For precise control, `miniconf_mqtt` exposes three explicit building blocks:

- `Startup`: fresh-session or resumed-session bring-up
- `Service`: bounded cooperative MM2 request service with non-MM2 passthrough
- `Publisher`: explicit retained republish for a leaf, subtree, or root

Typical flow:

1. construct MM2 state and session with `Miniconf::new(prefix, config)`
2. call `let event = session.connect(io).await?`
3. call `mm2.startup(&mut session, &settings, event)`
4. in steady state, call `mm2.serve(&mut session, &mut settings, on_unhandled)`
5. use `Publisher::root(Settings::SCHEMA)` or `Publisher::by_key(Settings::SCHEMA, key)` for explicit app-side retained
   republish

`Publisher::root(Settings::SCHEMA)` replaces the old full-tree `publish_all()` flow.

## Core contract

Simple helpers:

- `mm2.startup(...)` runs the MM2 work required by one `ConnectEvent` to completion.
- `mm2.serve(...)` waits until one `/set` has been applied and fully republished, or until
  one non-MM2 inbound publish has been handled by the callback and returned.
- both helpers are unbounded
- fresh-session startup may discard inbound publishes while bootstrapping
- `serve()` may discard inbound publishes that arrive while completing MM2 follow-up work
- use the explicit stepwise APIs below when that is not acceptable

Stepwise APIs:

- `Startup::step() -> Ok(true)` means startup is complete
- `Startup::step() -> Ok(false)` means it cannot make more immediate startup progress
- `Publisher::step() -> Ok(true)` means retained republish is complete
- `Service::step() -> Ok(true)` means no queued MM2 follow-up work remains

`Service` is the cooperative steady-state boundary:

- `ServiceEvent::Unhandled` means the caller still owns the non-MM2 publish and
  may route it elsewhere
- `ServiceEvent::Changed(changed)` means one `/set` changed local settings and queued authoritative
  MM2 follow-up work
- `ServiceEvent::Busy` means bounded service capacity was exhausted, so the MM2 request was
  rejected without mutating settings
- `ServiceEvent::Idle` means MM2 recognized the message and intentionally did nothing

Practical boundary:

- use `Session::poll()` to wait for any later session progress
- use `Session::recv()` when you specifically want the next inbound publish
- `Startup::step()` may consume and discard inbound publishes while bootstrapping
- `Publisher::step()` must not consume unrelated inbound publishes
- `Service::step()` must not consume unrelated inbound publishes
- after any `step()` returns `false`, wait for later session progress before retrying
- after `Publisher::step()` returns `false`, the caller must route any surfaced inbound publishes
  before retrying

Bounded cooperative serving:

```rust
let mut service = Service::<4>::new();

loop {
    let _empty = service.step(&mut mm2, &mut session, &settings).await?;

    if let Some(inbound) = session.poll().await? {
        match service.handle(&mut mm2, &mut settings, &inbound) {
            ServiceEvent::Unhandled => { /* app traffic */ }
            ServiceEvent::Changed(_) | ServiceEvent::Busy | ServiceEvent::Idle => {}
        }
    }
}
```

`Service` aftermath rules:

- successful `/set`: publish or clear the authoritative retained leaf, then optionally reply on
  the MQTT response topic
- failed `/set`: send only the optional error reply
- busy bounded service: reject without mutating local settings
- without `compat-settings-ingress`, failed `/set` never republishes `settings/...`

`Publisher`:

- `Publisher::root(Settings::SCHEMA)`: full retained settings mirror
- leaf key via `Publisher::by_key(Settings::SCHEMA, ...)`: publish or clear exactly that leaf
- inner key via `Publisher::by_key(Settings::SCHEMA, ...)`: recursively publish or clear descendant leaves

If a descendant leaf currently serializes as `Absent` or `Access`, `Publisher` clears that exact
retained `settings/...` leaf topic with an empty retained payload and a fresh `rev`.

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
- long-lived clients should ignore `settings/<path>` without `rev`

`set/<path>` accepts one JSON value for one leaf.

- success republishes authoritative retained `settings/<path>`
- if `Response Topic` is present, success also emits an `Ok` reply on `response`
- failure emits only an `Error` reply on `response`
- explicit replies are metadata-only; the authoritative applied value is always the retained
  `settings/<path>` publication

## Response metadata

Error replies currently carry MQTT v5 user properties:

- `code`
- `kind`
- `class`
- `error`
- optional `depth`

Success replies carry only `code=Ok`.

## Compatibility mode

Optional feature `compat-settings-ingress` is currently only retained as a compatibility feature
flag. It does not shape the core MM2 API described here.

## Limitations

- MM2 is small and opinionated. One MQTT prefix is assumed to have one authoritative device
  publisher.
- Publication is incremental, not atomic. Clients must treat retained `alive` as the authority
  for `epoch` and `schema_rev`.
- `Startup::step() -> Ok(true)` means no more immediate startup work remains. It does not wait
  for broker ACKs or `SUBACK`.
- `Publisher` prunes only leaves in the currently traversed schema subtree. It does not discover
  arbitrary retained topics left behind by older schema shapes.
