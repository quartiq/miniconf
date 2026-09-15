import type { MqttAuth } from "./mqtt-session";

const key = "miniconf-web.connection";

// One tab-local broker identity; never part of routes or permanent storage.
export function restoreAuth(broker: string): MqttAuth | undefined {
  try {
    const value = JSON.parse(sessionStorage.getItem(key) ?? "null");
    if (
      value?.broker === broker &&
      typeof value.username === "string" &&
      typeof value.password === "string"
    )
      return { username: value.username, password: value.password };
    sessionStorage.removeItem(key);
  } catch {
    // Storage may be unavailable, including for saved-file use.
  }
  return undefined;
}

export function rememberAuth(broker?: string, auth?: MqttAuth): boolean {
  try {
    if (broker && auth && (auth.username || auth.password))
      sessionStorage.setItem(
        key,
        JSON.stringify({
          broker,
          username: auth.username,
          password: auth.password,
        }),
      );
    else sessionStorage.removeItem(key);
    return true;
  } catch {
    return false;
  }
}
