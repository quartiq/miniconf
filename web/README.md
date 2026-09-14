# Miniconf MQTT Web Browser

Inspect and edit Miniconf devices over MQTT v5 WebSockets.

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

Enter a full WebSocket broker URL and a discovery filter such as `dt/sinara/+/+`, then press
Discover. `+` matches one level; `#` is unsupported. Connection-field edits take effect on
Discover, not while typing. Credentials use browser autocomplete and stay out of saved links.

Select a leaf, edit its JSON, and press Set or Ctrl/Cmd+Enter. The exact text is sent, without
rounding large integers. Incoming updates, reconnects and folding preserve your draft;
selecting another item replaces it. Use device value discards the draft.

If a request's outcome is unknown, inspect the device value before sending again. Set is
disabled while disconnected; use Retry if automatic recovery stops.

## Retained-topic cleanup

**Prune (N)** appears when stale retained topics are observed for an alive device.
Click it to clear those topics.
This removes stale broker storage across the device prefix, even when browsing a subtree;
valid settings are kept. An interrupted clear may have partly completed.
Whole-prefix cleanup stays in the Python client.

## Test

```sh
npm run check
npm test
npm run format:check
npm run build
npm run test:browser
```

The browser test needs Chrome/Chromium (`CHROME_BIN` can select it) and uses a local fixture,
not hardware. It checks both served and `file://` builds.

Live broker smoke test:

```sh
MINICONF_WEB_BROKER=wss://mqtt.quartiq.de \
MINICONF_WEB_FILTER='dt/sinara/+/+' \
npm run test:integration
```
