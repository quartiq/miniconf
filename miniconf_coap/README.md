# `miniconf_coap`

Expose a `miniconf` tree as CoAP leaf resources: GET reads a value, PUT assigns it.
No allocator is required. The application supplies settings and response storage;
the CoAP server handles routing, security and message delivery.

## Leaf values

`JsonValueRoute` serves JSON. Enable `cbor` for `CborValueRoute`.

```rust
# #[cfg(feature = "json-core")]
# {
use miniconf::Tree;
use miniconf_coap::{JsonValueRoute, RequestParts};

#[derive(Default, Tree)]
struct Settings {
    enabled: bool,
}

let route = JsonValueRoute::json("/settings");
let mut settings = Settings::default();
let mut buffer = [0; 128];
let request = RequestParts::new(
    coap_numbers::code::GET,
    &["settings", "enabled"],
    None,
    None,
    b"",
).unwrap();

let response = route.handle(&request, &mut settings, &mut buffer).response().unwrap();
assert_eq!(response.payload, b"false");
# }
```

Use `RequestParts::from_message()` with an existing CoAP message.
PUT requires the route's Content-Format. A successful assignment returns
`Outcome::Written { key, response }`, even if the value was already equal.
Leaf decoding finishes before assignment; custom hooks own their side effects.

Value responses have `Max-Age: 0`. Errors carry CoAP status codes and optional
UTF-8 diagnostics without Content-Format or a machine-readable error format.
URI segments map directly to Miniconf keys; a segment containing `/` is rejected.

## Schema and discovery

`SchemaRoute::new("/schema", Settings::SCHEMA, 128)` serves a JSON manifest at
`/schema` and newline-delimited compact schema pages at `/schema/{page}`.
With `miniconf::TreeSchema` in scope, `Settings::SCHEMA` supplies the schema.
Buffers must fit the route's fixed page budget and the manifest.

With `coap-handler`, mount `MiniconfCoapHandler` and `SchemaCoapHandler` beneath
your router's prefixes. Their discovery records support CoRE Link Format.
The value adapter exposes its owned tree through `settings()` and `settings_mut()`.
PUT runs during request extraction;
`ValueRequest::Written(key)` lets an application react before response construction.
Applications remain responsible for hardware application and duplicate requests.

## Try it

```sh
cargo run -p miniconf_coap --example coap_server --features coap-handler
aiocoap-client 'coap://[::1]:56830/settings/enabled'
aiocoap-client -m PUT --content-format application/json --payload true 'coap://[::1]:56830/settings/enabled'
aiocoap-client 'coap://[::1]:56830/.well-known/core'
```

The example uses `embedded-nal-minimal-coapserver` and prints successful assignments.
It listens on all interfaces; an optional argument changes the port.
Use it for local experiments: the server has no security or duplicate suppression.

## Features and bounds

- `json-core` (default): JSON values and compact schema.
- `cbor`: CBOR values.
- `coap-handler`: handler adapters and discovery.

The `MAX_*` constants bound paths, keys, schema definitions and adapter responses.
Adapter construction panics if the schema exceeds `MAX_DEPTH`.
