import {
  DiscoverySession,
  PrefixSession,
  type DiscoverySessionCallbacks,
  type PrefixSessionCallbacks,
  type SessionStatus,
} from "./backend";
import type { MqttAuth } from "./mqtt-session";
import type { AppRoute } from "./routes";
import { rememberAuth } from "./session-auth";

type Callbacks = Omit<PrefixSessionCallbacks, "status"> &
  DiscoverySessionCallbacks;

// Owns the applied connection; route history and editable form state stay in the UI.
export class Connection {
  session = $state.raw<DiscoverySession | PrefixSession>();
  credentials = $state<(MqttAuth & { broker: string }) | undefined>();
  status = $state<SessionStatus>({ state: "idle" });
  ready = $state(false);
  notice = $state("");
  private lifetime = new AbortController();

  constructor(private readonly callbacks: Callbacks) {}

  get signal(): AbortSignal {
    return this.lifetime.signal;
  }

  close(): void {
    this.lifetime.abort();
    this.session?.close();
    this.session = undefined;
    this.ready = false;
  }

  async open(route: AppRoute, credentialsRequired = false): Promise<void> {
    this.close();
    this.lifetime = new AbortController();
    const signal = this.signal;
    this.notice = "";
    if (this.credentials?.broker !== route.broker) {
      this.credentials = undefined;
      rememberAuth();
    }
    if (route.page === "landing") {
      this.noteStatus({ state: "idle" });
      return;
    }
    if (!this.credentials && credentialsRequired) {
      this.noteStatus({ state: "credentials" });
      return;
    }
    this.noteStatus({ state: "connecting" });
    const auth = this.credentials;
    const options = { auth, signal };
    try {
      const next =
        route.page === "browse"
          ? await PrefixSession.connect(
              route.broker,
              route.activePrefix,
              route.subtreePath,
              {
                alive: (value) => {
                  if (!signal.aborted) this.callbacks.alive(value);
                },
                schema: (schema, root) => {
                  if (!signal.aborted) this.callbacks.schema(schema, root);
                },
                settings: (value) => {
                  if (!signal.aborted) this.callbacks.settings(value);
                },
                pruning: (value) => {
                  if (!signal.aborted) this.callbacks.pruning?.(value);
                },
                status: (status, ready) => {
                  if (!signal.aborted) this.noteStatus(status, ready);
                },
              },
              options,
            )
          : await DiscoverySession.connect(
              route.broker,
              route.discoveryFilter,
              {
                prefixes: (value) => {
                  if (!signal.aborted) this.callbacks.prefixes(value);
                },
                status: (status) => {
                  if (!signal.aborted) this.noteStatus(status);
                },
              },
              options,
            );
      if (signal.aborted) {
        next.close();
        return;
      }
      this.session = next;
      if (
        !rememberAuth(route.broker, auth) &&
        (auth?.username || auth?.password)
      )
        this.notice =
          "Credentials will not survive reload: browser storage unavailable";
    } catch (error) {
      if (!signal.aborted)
        this.noteStatus({
          state: "failed",
          error: error instanceof Error ? error.message : String(error),
        });
    }
  }

  private noteStatus(next: SessionStatus, ready = false): void {
    this.ready = ready;
    const detail = "error" in next ? next.error : "";
    const previous = "error" in this.status ? this.status.error : "";
    if (this.status.state === next.state && previous === detail) return;
    this.status = next;
    this.callbacks.status(next);
  }
}
