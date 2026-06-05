export type StoredAuth = {
  username: string;
  password: string;
};

function authKey(broker: string): string {
  return `miniconf-web-auth:${broker}`;
}

export function loadAuth(broker: string): StoredAuth {
  try {
    const raw = sessionStorage.getItem(authKey(broker));
    return raw ? JSON.parse(raw) : { username: "", password: "" };
  } catch {
    return { username: "", password: "" };
  }
}

export function saveAuth(broker: string, auth: StoredAuth): void {
  sessionStorage.setItem(authKey(broker), JSON.stringify(auth));
}
