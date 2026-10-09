# `miniconf_mqtt`

`miniconf_mqtt` exposes a [`miniconf`](../miniconf/README.md) tree over MQTT using
[`minimq`](https://docs.rs/minimq).
It owns Miniconf MQTT protocol state; the caller owns the MQTT session, live connection, and
settings tree.

## Limitations

- Use one authoritative device per MQTT prefix.
- The broker must support QoS 1; leave MiniMQ's automatic QoS downgrade disabled.
- Schema/settings publication is incremental, not atomic; `alive` announces completed startup.
- Retained recovery uses a quiescence heuristic, not a storage transaction, and can trigger setter
  side effects.
- The device clears only leaves in the traversed schema. Pruning retained topics left behind by
  older schemas remains a client/tooling operation.

## Quick start

See the runnable example in [examples/miniconf.rs](examples/miniconf.rs).

Construct the protocol state and session with `Miniconf::new(prefix, config)`, then connect with
`session.connect(io).await?`. After each connection, run
`miniconf.startup(&mut connection, &settings).await?`. In steady state,
`miniconf.serve(&mut connection, &mut settings, on_unhandled).await?` returns after a successful
leaf write and its protocol follow-up, or after the callback handles non-Miniconf traffic.

`on_unhandled` is synchronous and called at most once per `serve()` call. Return owned application
data through `Event::Unhandled` when handling it requires an await.

These helpers run to completion and may discard inbound traffic during protocol follow-up.

## Cooperative serving

For precise control, `miniconf_mqtt` exposes four explicit building blocks:

- `Startup`: publish schema/settings and establish request handling after a connection
- `Service`: handle requests with a bounded queue of protocol follow-ups
- `Publisher`: publish a leaf, subtree, or root after application-side changes
- `LoadRetained`: recover retained settings before the first startup

`Startup::step()`, `Publisher::step()`, and `Service::step()` leave inbound routing to the caller.
`Ok(true)` means the workflow is complete or the service queue is empty. After `Ok(false)`, drive
`Connection::poll()`, route any inbound publication, and retry. `Startup::run()` and
`Publisher::run()` consume connection progress themselves and may discard inbound publications.

```rust
let mut service = Service::<4>::new();

loop {
    service.step(&miniconf, &mut connection, &settings).await?;

    if let Some(inbound) = connection.poll().await? {
        match service.handle(&miniconf, &mut settings, &inbound) {
            ServiceEvent::Unhandled => { /* app traffic */ }
            ServiceEvent::Changed(_) | ServiceEvent::Busy | ServiceEvent::Idle => {}
        }
    }
}
```

- `Changed(changed)`: a leaf write was accepted and protocol follow-up queued
- `Busy`: the queue was full; settings were not mutated
- `Idle`: no leaf write was accepted; an error reply or mirror repair may be queued
- `Unhandled`: route the publication to another handler

## Retained recovery

Retained settings recovery is a cold-boot step:

```rust
let mut load = miniconf_mqtt::LoadRetained::new();
load.run(&miniconf, &mut connection, &mut settings).await?;

let mut startup = miniconf_mqtt::Startup::connected(&mut miniconf);
startup.run(&mut miniconf, &mut connection, &settings).await?;
```

`LoadRetained` applies only retained `settings/<leaf>` publications with `auth=""`. It waits for
quiet after the last accepted value, then unsubscribes. Stale topics, missing `auth`, empty
payloads, and invalid JSON are ignored. Applying values can trigger normal setter side effects.

Use it only before the first Miniconf MQTT startup of a device process. On a device reconnect or
network glitch, keep the live settings in RAM authoritative and call
`miniconf.startup(...)`; it reads the connect event from the live connection:

- `ConnectEvent::Connected`: publish schema/settings, subscribe to `set/#`, and publish `alive`
- `ConnectEvent::Reconnected`: the broker resumed the MQTT session. If startup previously completed,
  Miniconf republishes only `alive`; otherwise it restarts schema/settings synchronization
  from current settings

## Protocol details

The Miniconf MQTT protocol version is 1:

- retained `<prefix>/alive` publishes a compact device manifest
- retained `<prefix>/schema/<n>` publishes paged compact schemata
- retained `<prefix>/settings<path>` publishes authoritative leaf values
- non-retained `<prefix>/set<path>` accepts explicit leaf mutation requests
- replies go to the requester's MQTT Response Topic

`<path>` is empty for a root leaf; otherwise it starts with `/`.

## Manifest

The retained `alive` payload is JSON:

```json
{"proto":1,"epoch":1,"schema_rev":12345678,"pages":7}
```

- `proto` is the Miniconf MQTT protocol version; clients should reject unsupported values
- `epoch` counts full startup cycles and resets with a new Miniconf instance
- `schema_rev` identifies the current schema page generation
- `pages` is the number of retained schema pages

`alive` is published after startup's schema/settings acknowledgements and request subscription
complete. Publication is incremental, not atomic. Clients may reuse a parsed schema if
`schema_rev` is unchanged.

The retained will clears `alive` on an ungraceful disconnect.

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

Authoritative retained `settings/<path>` publications carry exactly one MQTT v5 user property
`auth=""`. Clients should ignore settings publications without valid `auth`.

Clients read `alive`, assemble its schema pages, and subscribe to the authoritative settings mirror.

`set/<path>` accepts one JSON leaf value. Requests with the retain flag are rejected.

- success republishes authoritative retained `settings/<path>`
- if `Response Topic` is present, success also emits an `Ok` reply there
- failure emits only an optional `Error` reply there
- the authoritative value is published on `settings/<path>` before the optional success reply

Full startup and `Publisher::root(Settings::SCHEMA)` publish every schema leaf.
`Publisher::by_key(Settings::SCHEMA, key)` publishes one leaf or all descendant leaves of a subtree.
Leaves that serialize as `Absent` or `Access` clear their retained topic with an empty payload and
`auth=""`. Pruning topics left behind by older schemas remains a client/tooling operation.

For compatibility with simple MQTT tools, an application may subscribe to `settings/#` itself
using `RetainHandling::Never` and `retain_as_published()` and route those publishes through
`Service`. Only non-retained, no-`auth` leaf publishes are treated as requests. Authoritative
retained publications are applied only by cold-boot `LoadRetained` recovery. Failed compatibility
writes are overwritten with the current authoritative leaf value.

## Response metadata

Error replies carry MQTT v5 user properties:

- `code`
- `kind`
- `class`
- `error`
- optional `depth`

Success replies carry `code=Ok` and an empty payload. Errors carry `code=Error` and diagnostic text.
