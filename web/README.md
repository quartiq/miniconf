# Miniconf MQTT Web Browser

Proof-of-principle browser app for `miniconf_mqtt` targets.

The app talks directly to an MQTT broker through MQTT v5 over WebSockets.
Configure a broker WebSocket listener and pass its
authority in the URL.

## Run

```sh
npm install
npm run dev
```

Open the printed local URL. Discovery uses hash paths so the static build can be served by
hosts without route fallback support:

```text
#/discover/mqtt:8083/dt/sinara/+/+
```

Routes:

- `#/discover/{broker}/{wildcard}` discovers device prefixes. `{broker}` is the WebSocket broker authority; `mqtt:8083` means `ws://mqtt:8083`.
- `#/browse/{broker}/{prefix}` opens an active prefix.
- `?path=` selects an active-prefix subtree. The default is the empty root path.

Discovery lists all matching prefixes as links. Opening a prefix link enters the browse
view, loads the retained /alive manifest, paged schema, retained settings below the URL subtree, and keeps
listening for authoritative `/alive` and `/settings` updates.

The status line is a collapsed log. Open it to inspect recent coalesced protocol/UI events, or
append `?log=1` to the page URL to open the log from startup.

Leaf values are edited as JSON and submitted through `/set`. The explicit response reports
whether the request was accepted; the retained `/settings` publication remains the authoritative
applied value.

Optional MQTT username/password fields are stored in `sessionStorage` per broker and are never
placed in route URLs. A direct browse link can use stored credentials from the same browser session;
otherwise it connects anonymously.

The client accepts the miniconf_mqtt wire protocol `proto=1` alive manifests and ignores retained `/settings`
publications without exactly one empty `auth` user property.

## Static Hosting

The app is a static Vite build and can be deployed to GitHub Pages. The repository includes a
manual/push workflow for Pages. For project pages, the workflow builds with:

```sh
BASE_PATH=/<repo-name>/ npm run build
```

The app uses hash routing, so deep links work without `404.html` fallbacks.

Public HTTPS hosting requires brokers reachable through `wss://` with browser-valid TLS. Browser
pages served over HTTPS cannot use insecure `ws://` brokers. Broker authentication and ACLs remain
the access-control boundary; publishing this static app does not protect a broker that is already
publicly reachable.

The included HTML uses a pragmatic meta CSP: scripts/styles load from the static site,
`worker-src` allows MQTT.js' browser worker, and browser connections are allowed to `ws:`/`wss:`
so users can choose brokers. Tighten `connect-src` to known broker origins for a locked-down
deployment.

## Test

Run the static and fixture-backed checks:

```sh
npm run check
npm test
```

To include a live WebSocket broker smoke test:

```sh
MINICONF_WEB_BROKER=ws://mqtt:8083 \
MINICONF_WEB_FILTER='dt/sinara/+/+' \
npm test
```
