# Miniconf MQTT Web Browser

Browser UI for `miniconf_mqtt` targets over MQTT v5 WebSockets.

## Run

```sh
npm install
npm run dev
```

Open the printed local URL. Development uses normal Vite modules and HMR.

## Build

```sh
npm run build
```

The production artifact is a single self-contained `dist/index.html`. It can be deployed to
GitHub Pages, served from any static host, or opened as a local file.

No local tooling is needed to get the bundle: open <https://miniconf.quartiq.de/>, save the page as
HTML, and open the saved file in the browser.

## Routes

- `#/discover/{broker}/{wildcard}` discovers device prefixes.
- `#/browse/{broker}/{prefix}` opens an active prefix.
- Hash query `endpoint=` preserves an optional WebSocket path and query.
- Hash query `path=` selects a subtree. The default is the empty root path.
- Document query `log=1` opens the log from startup, for example `?log=1#/discover/...`.

`{broker}` is the WebSocket broker authority. `mqtt:8083` means `ws://mqtt:8083`;
`wss+broker.example:8084` means `wss://broker.example:8084`.
The connection form itself accepts full `ws://` and `wss://` URLs such as
`wss://mqtt.quartiq.de:1239/path/to/socket` and starts empty rather than guessing a broker.

## Browser/Broker Matrix

| App origin | Broker | Chromium | Firefox/Safari | Notes |
| --- | --- | --- | --- | --- |
| `http://localhost` or private `http://` | private `ws://` | Works | Works | Best for LAN brokers and development. |
| `file://` | private `ws://` | Works | Works | Use `dist/index.html`. |
| public `http://` | private `ws://` | Usually blocked or permission-gated | Works today | Chromium Local Network Access applies. |
| public `https://` | private `ws://` | Blocked | Blocked | Mixed content. Use `wss://` or local origin. |
| public `https://` | public `wss://` with valid cert | Works | Works | Clean public deployment. |
| public `https://` | public reverse proxy `wss://` -> private `ws://` | Works | Works | Proxy owns TLS and access control. |

## Use

Discovery lists matching prefixes. Selecting a prefix loads the retained `/alive` manifest,
paged schema, retained settings for the selected subtree, and live `/alive` and `/settings`
updates.

Leaf values are edited as JSON and submitted through `/set`. `/set` responses report request
acceptance; `/settings` publications remain the authoritative applied values.
The empty Miniconf path is the schema root; `/` is its empty-name child.

Optional MQTT credentials are never placed in route URLs or application-managed storage. The
connection form uses standard browser autocomplete so a password manager can remember them when
the user chooses.

## Test

```sh
npm run check
npm test
```

Live broker smoke test:

```sh
MINICONF_WEB_BROKER=wss://mqtt.quartiq.de \
MINICONF_WEB_FILTER='dt/sinara/+/+' \
npm test
```
