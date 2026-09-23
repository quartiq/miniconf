import { afterEach, expect, it, vi } from "vitest";
import { DiscoverySession, type DiscoverySessionCallbacks } from "./backend";
import { Connection } from "./connection.svelte";
import type { AppRoute } from "./routes";
import { rememberAuth } from "./session-auth";

vi.mock("./session-auth", () => ({ rememberAuth: vi.fn(() => true) }));
afterEach(() => vi.restoreAllMocks());

const route: AppRoute = {
  page: "discover",
  broker: "ws://first",
  discoveryFilter: "dt/+",
  activePrefix: "",
  subtreePath: "",
};
function callbacks() {
  return {
    alive: vi.fn(),
    schema: vi.fn(),
    settings: vi.fn(),
    prefixes: vi.fn(),
    status: vi.fn(),
  };
}

it("closes superseded sessions and ignores their late callbacks and completion", async () => {
  const events = callbacks();
  const connection = new Connection(events);
  let obsoleteCallbacks!: DiscoverySessionCallbacks;
  let finish!: (value: DiscoverySession) => void;
  const current = { close: vi.fn() } as unknown as DiscoverySession;
  vi.spyOn(DiscoverySession, "connect")
    .mockImplementationOnce((_broker, _filter, cb) => {
      obsoleteCallbacks = cb;
      return new Promise((resolve) => {
        finish = resolve;
      });
    })
    .mockResolvedValueOnce(current);
  const opening = connection.open(route);
  const oldSignal = connection.signal;
  await connection.open({ ...route, broker: "ws://second" });
  expect(oldSignal.aborted).toBe(true);
  obsoleteCallbacks.prefixes([]);
  obsoleteCallbacks.status({ state: "failed", error: "obsolete" });
  const obsolete = { close: vi.fn() };
  finish(obsolete as unknown as DiscoverySession);
  await opening;
  expect(obsolete.close).toHaveBeenCalledOnce();
  expect(events.prefixes).not.toHaveBeenCalled();
  expect(connection.session).toBe(current);
  expect(connection.status.state).not.toBe("failed");
  connection.close();
  expect(current.close).toHaveBeenCalledOnce();
});

it("forgets another broker's credentials and waits when authentication is required", async () => {
  const connection = new Connection(callbacks());
  connection.credentials = {
    broker: route.broker,
    username: "alice",
    password: "secret",
  };
  const connect = vi.spyOn(DiscoverySession, "connect");
  await connection.open({ ...route, broker: "ws://second" }, true);
  expect(connection.credentials).toBeUndefined();
  expect(rememberAuth).toHaveBeenCalledWith();
  expect(connection.status.state).toBe("credentials");
  expect(connect).not.toHaveBeenCalled();
});
