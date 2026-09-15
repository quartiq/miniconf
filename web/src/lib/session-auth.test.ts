import { afterEach, expect, it, vi } from "vitest";
import { rememberAuth, restoreAuth } from "./session-auth";

afterEach(() => vi.unstubAllGlobals());

it("retains only the current broker and clears anonymous credentials", () => {
  const values = new Map<string, string>();
  vi.stubGlobal("sessionStorage", {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
  });
  const auth = { username: "user", password: "secret" };
  expect(rememberAuth("wss://one/mqtt", auth)).toBe(true);
  expect(restoreAuth("wss://one/mqtt")).toEqual(auth);
  expect(restoreAuth("wss://one/other")).toBeUndefined();
  expect(restoreAuth("wss://one/mqtt")).toBeUndefined();
  rememberAuth("wss://one/mqtt", auth);
  rememberAuth("wss://one/mqtt", { username: "", password: "" });
  expect(values.size).toBe(0);
  rememberAuth("wss://one/mqtt", auth);
  rememberAuth();
  expect(values.size).toBe(0);
});

it("allows connection use when storage is unavailable", () => {
  const denied = () => {
    throw new DOMException("Storage denied", "SecurityError");
  };
  vi.stubGlobal("sessionStorage", {
    getItem: denied,
    setItem: denied,
    removeItem: denied,
  });
  expect(restoreAuth("wss://one")).toBeUndefined();
  expect(
    rememberAuth("wss://one", { username: "user", password: "secret" }),
  ).toBe(false);
});
