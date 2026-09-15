export function randomId(): string {
  return (
    crypto.randomUUID?.() ??
    Array.from(crypto.getRandomValues(new Uint32Array(4)), (word) =>
      word.toString(16).padStart(8, "0"),
    ).join("")
  );
}
