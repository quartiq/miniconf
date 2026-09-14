import type { Schema } from "./schema";

// Retained responses are transient; valid leaf settings and requests are kept.
// Before schema readiness, collect topic names without interpreting payloads.
export function staleTopic(
  prefix: string,
  schema: Schema | undefined,
  topic: string,
): boolean {
  for (const namespace of ["settings", "set"]) {
    const base = `${prefix}/${namespace}`;
    if (topic !== base && !topic.startsWith(`${base}/`)) continue;
    if (!schema) return true;
    try {
      return schema.node(topic.slice(base.length)).kind !== "leaf";
    } catch {
      return true;
    }
  }
  const response = `${prefix}/response`;
  return topic === response || topic.startsWith(`${response}/`);
}
