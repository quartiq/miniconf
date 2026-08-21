export type AppRoute = {
  page: "landing" | "discover" | "browse";
  broker: string;
  discoveryPattern: string;
  activePrefix: string;
  subtreePath: string;
};

export const DEFAULT_FILTER = "dt/sinara/+/+";

function topicPath(value: string): string {
  return value.split("/").map((segment) => encodeURIComponent(segment).replace(/%2B/gi, "+")).join("/");
}

function topicFromSegments(segments: string[]): string {
  return segments.map(decodeURIComponent).join("/");
}

function brokerRoute(broker: string): { token: string; endpoint: string } {
  const url = brokerUrl(broker);
  if (url.username || url.password) {
    throw new Error("Enter broker credentials in the username and password fields");
  }
  return {
    token: url.protocol === "wss:" ? `wss+${url.host}` : url.host,
    endpoint: url.pathname === "/" && !url.search && !url.hash
      ? ""
      : `${url.pathname}${url.search}${url.hash}`,
  };
}

function brokerFromRoute(token: string, endpoint: string): string | undefined {
  if (endpoint && !endpoint.startsWith("/")) return undefined;
  try {
    const url = brokerUrl(
      `${token.startsWith("wss+") ? `wss://${token.slice(4)}` : `ws://${token}`}${endpoint}`,
    );
    if (url.username || url.password) return undefined;
    return url.pathname === "/" && !url.search && !url.hash
      ? `${url.protocol}//${url.host}`
      : url.href;
  } catch {
    return undefined;
  }
}

function brokerUrl(broker: string): URL {
  const url = new URL(broker);
  if (url.protocol !== "ws:" && url.protocol !== "wss:") {
    throw new Error("Broker URL must start with ws:// or wss://");
  }
  return url;
}

function hashRoute(location: Pick<Location, "hash">): { path: string; search: string } {
  const hash = location.hash.startsWith("#") ? location.hash.slice(1) : location.hash;
  const [path, search = ""] = hash.split("?");
  return { path: path || "/", search: search ? `?${search}` : "" };
}

export function readRoute(location: Pick<Location, "hash">): AppRoute {
  try {
    const route = hashRoute(location);
    const params = new URLSearchParams(route.search);
    const parts = route.path.split("/").filter(Boolean);
    if (parts.length >= 2 && (parts[0] === "discover" || parts[0] === "browse")) {
      const [action, broker, ...topic] = parts;
      const routeBroker = brokerFromRoute(broker, params.get("endpoint") ?? "");
      const routeTopic = topicFromSegments(topic);
      if (!routeBroker || (action === "browse" && !routeTopic)) {
        return landingRoute();
      }
      return {
        page: action,
        broker: routeBroker,
        discoveryPattern: action === "discover"
          ? routeTopic || DEFAULT_FILTER
          : params.get("discover") || DEFAULT_FILTER,
        activePrefix: action === "browse" ? routeTopic : "",
        subtreePath: params.get("path") ?? "",
      };
    }
  } catch {
    // Malformed hashes should not break the static app shell.
  }
  return landingRoute();
}

function landingRoute(): AppRoute {
  return {
    page: "landing",
    broker: "",
    discoveryPattern: DEFAULT_FILTER,
    activePrefix: "",
    subtreePath: "",
  };
}

export function discoveryPath(broker: string, discoveryPattern: string): string {
  const { token, endpoint } = brokerRoute(broker);
  const query = endpoint ? `?${new URLSearchParams({ endpoint })}` : "";
  return `#/discover/${token}/${topicPath(discoveryPattern)}${query}`;
}

export function browsePath(
  broker: string,
  prefix: string,
  subtreePath = "",
  discoveryPattern = DEFAULT_FILTER,
): string {
  const { token, endpoint } = brokerRoute(broker);
  const params = new URLSearchParams();
  if (endpoint) {
    params.set("endpoint", endpoint);
  }
  if (subtreePath) {
    params.set("path", subtreePath);
  }
  if (discoveryPattern !== DEFAULT_FILTER) {
    params.set("discover", discoveryPattern);
  }
  const query = params.toString();
  return `#/browse/${token}/${topicPath(prefix)}${query ? `?${query}` : ""}`;
}
