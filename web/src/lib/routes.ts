export type AppRoute = {
  page: "landing" | "discover" | "browse";
  broker: string;
  discoveryPattern: string;
  activePrefix: string;
  subtreePath: string;
};

export const DEFAULT_BROKER = "ws://mqtt:8083";
export const DEFAULT_SECURE_BROKER = "wss://mqtt:8084";
export const DEFAULT_FILTER = "dt/sinara/+/+";

function defaultBroker(protocol = globalThis.location?.protocol): string {
  return protocol === "https:" ? DEFAULT_SECURE_BROKER : DEFAULT_BROKER;
}

function topicPath(value: string): string {
  return value.split("/").map((segment) => encodeURIComponent(segment).replace(/%2B/gi, "+")).join("/");
}

function topicFromSegments(segments: string[]): string {
  return segments.map(decodeURIComponent).join("/");
}

function brokerToken(broker: string): string {
  const normalized = broker.includes("://") ? broker : `ws://${broker}`;
  const url = safeUrl(normalized) ?? new URL(defaultBroker());
  return url.protocol === "wss:" ? `wss+${url.host}` : url.host;
}

function brokerFromToken(token: string): string | undefined {
  if (token.startsWith("wss+")) {
    return safeBroker(`wss://${token.slice(4)}`);
  }
  return safeBroker(`ws://${token}`);
}

function safeBroker(broker: string): string | undefined {
  return safeUrl(broker) ? broker : undefined;
}

function safeUrl(url: string): URL | undefined {
  try {
    return new URL(url);
  } catch {
    return undefined;
  }
}

function hashRoute(location: Pick<Location, "hash">): { path: string; search: string } {
  const hash = location.hash.startsWith("#") ? location.hash.slice(1) : location.hash;
  const [path, search = ""] = hash.split("?");
  return { path: path || "/", search: search ? `?${search}` : "" };
}

export function readRoute(location: Pick<Location, "hash"> & Partial<Pick<Location, "protocol">>): AppRoute {
  const defaultBrokerUrl = defaultBroker(location.protocol);
  try {
    const route = hashRoute(location);
    const params = new URLSearchParams(route.search);
    const parts = route.path.split("/").filter(Boolean);
    if (parts.length >= 2 && (parts[0] === "discover" || parts[0] === "browse")) {
      const [action, broker, ...topic] = parts;
      const routeBroker = brokerFromToken(broker);
      if (!routeBroker) {
        return landingRoute(defaultBrokerUrl);
      }
      return {
        page: action,
        broker: routeBroker,
        discoveryPattern: action === "discover"
          ? topicFromSegments(topic) || DEFAULT_FILTER
          : params.get("discover") || DEFAULT_FILTER,
        activePrefix: action === "browse" ? topicFromSegments(topic) : "",
        subtreePath: params.get("path") ?? "",
      };
    }
  } catch {
    // Malformed hashes should not break the static app shell.
  }
  return landingRoute(defaultBrokerUrl);
}

function landingRoute(broker: string): AppRoute {
  return {
    page: "landing",
    broker,
    discoveryPattern: DEFAULT_FILTER,
    activePrefix: "",
    subtreePath: "",
  };
}

export function discoveryPath(broker: string, discoveryPattern: string): string {
  return `#/discover/${brokerToken(broker)}/${topicPath(discoveryPattern)}`;
}

export function browsePath(
  broker: string,
  prefix: string,
  subtreePath = "",
  discoveryPattern = DEFAULT_FILTER,
): string {
  const params = new URLSearchParams();
  if (subtreePath) {
    params.set("path", subtreePath);
  }
  if (discoveryPattern !== DEFAULT_FILTER) {
    params.set("discover", discoveryPattern);
  }
  const query = params.toString();
  return `#/browse/${brokerToken(broker)}/${topicPath(prefix)}${query ? `?${query}` : ""}`;
}
