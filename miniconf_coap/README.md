# `miniconf_coap`

`miniconf_coap` exposes selected [`miniconf`](../miniconf/README.md) trees as
CoAP resources. It is sessionless: applications keep ownership of CoAP sockets,
message IDs, tokens, routing, retransmission, unrelated resources, and the
settings tree unless they opt into the `coap-handler` adapter.

## Routes

- `JsonValueRoute::json("/settings")` serves leaf values as JSON.
- `CborValueRoute::cbor("/settings")` serves leaf values as CBOR when the
  `cbor` feature is enabled.
- `SchemaRoute::new("/schema", Settings::SCHEMA)` serves a JSON manifest at
  `/schema` and newline-delimited compact schema pages at `/schema/{page}`.

The cooperative routes borrow settings for each request:

```rust
# #[cfg(feature = "json-core")]
# {
use miniconf::{Tree, TreeSchema};
use miniconf_coap::{JsonValueRoute, Outcome, RequestParts, SchemaRoute};

#[derive(Default, Tree)]
struct Settings {
    enabled: bool,
}

let values = JsonValueRoute::json("/settings");
let schema = SchemaRoute::new("/schema", Settings::SCHEMA);
let mut settings = Settings::default();
let mut response = [0; 128];

let request = RequestParts::new(
    coap_numbers::code::GET,
    &["settings", "enabled"],
    None,
    None,
    b"",
)
.unwrap();

match values.handle(&request, &mut settings, &mut response) {
    Outcome::Handled(response) => assert_eq!(response.payload, b"false"),
    _ => unreachable!(),
}

let request = RequestParts::new(coap_numbers::code::GET, &["schema"], None, None, b"").unwrap();
assert!(schema.handle(&request, &mut response).response().is_some());
# }
```

## `coap-handler`

With the `coap-handler` feature, `MiniconfCoapHandler` and `SchemaCoapHandler`
adapt the routes to the `coap-handler` ecosystem. `MiniconfCoapHandler` owns its
settings because `coap-handler::Handler` has no per-request application context;
use `ValueRoute` directly when other components need cooperative access to the
same settings tree.

## Features

- `json-core` (default): JSON value routes and compact schema serving.
- `cbor`: CBOR value routes and concise problem details.
- `coap-handler`: adapters and CoRE Link Format discovery records.
