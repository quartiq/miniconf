export type CompactRef = number | { r: number; m?: unknown };

export type CompactDef = {
  i?:
    | { k: "n"; c: Record<string, CompactRef> }
    | { k: "d"; c: CompactRef[] }
    | { k: "h"; c: CompactRef; l: number };
  m?: unknown;
  s?: unknown;
};

export type SchemaKind = "leaf" | "named" | "numbered" | "homogeneous";

export type SchemaNode = {
  path: string;
  kind: SchemaKind;
  node?: unknown;
  edge?: unknown;
  sem?: unknown;
  children: SchemaChild[];
};

export type SchemaChild = {
  name: string;
  path: string;
  edge?: unknown;
};

function validatePath(path: string): string {
  if (path === "") {
    return "";
  }
  if (!path.startsWith("/")) {
    throw new Error("Path must be empty or start with '/'");
  }
  return path;
}

function refId(ref: CompactRef): number {
  return typeof ref === "number" ? ref : ref.r;
}

function refMeta(ref: CompactRef): unknown {
  return typeof ref === "number" ? undefined : ref.m;
}

export class Schema {
  readonly rev: number;
  readonly defs: CompactDef[];
  readonly root: number;

  constructor(defs: CompactDef[], rev: number) {
    if (defs.length === 0) {
      throw new Error("Schema has no definitions");
    }
    this.defs = defs;
    this.rev = rev;
    this.root = defs.length - 1;
  }

  path(path: string): string {
    const normalized = validatePath(path);
    this.resolve(normalized);
    return normalized;
  }

  node(path = ""): SchemaNode {
    const normalized = this.path(path);
    const { id, childRef } = this.resolve(normalized);
    const def = this.defs[id];
    const children = this.childEntries(normalized, id).map(({ name, ref }) => ({
      name,
      path: normalized ? `${normalized}/${name}` : `/${name}`,
      ...(refMeta(ref) === undefined ? {} : { edge: refMeta(ref) }),
    }));
    return {
      path: normalized,
      kind: this.kindFor(def),
      ...(def.m === undefined ? {} : { node: def.m }),
      ...(childRef === undefined || refMeta(childRef) === undefined
        ? {}
        : { edge: refMeta(childRef) }),
      ...(def.s === undefined ? {} : { sem: def.s }),
      children,
    };
  }

  children(path = ""): SchemaNode[] {
    return this.node(path).children.map((child) => this.node(child.path));
  }

  walk(path = ""): SchemaNode[] {
    const root = this.node(path);
    return [root, ...root.children.flatMap((child) => this.walk(child.path))];
  }

  kind(path = ""): SchemaKind {
    return this.node(path).kind;
  }

  private resolve(path: string): { id: number; childRef?: CompactRef } {
    let id = this.root;
    let childRef: CompactRef | undefined;
    if (!path) {
      return { id };
    }
    for (const part of path.slice(1).split("/")) {
      const entries = this.childEntries(path, id);
      const entry = entries.find((candidate) => candidate.name === part);
      if (!entry) {
        throw new Error(`Unknown schema path: ${path}`);
      }
      childRef = entry.ref;
      id = refId(entry.ref);
      if (id < 0 || id >= this.defs.length) {
        throw new Error(`Invalid schema reference ${id} in ${path}`);
      }
    }
    return { id, childRef };
  }

  private childEntries(_path: string, id: number): { name: string; ref: CompactRef }[] {
    const internal = this.defs[id].i;
    if (!internal) {
      return [];
    }
    switch (internal.k) {
      case "n":
        return Object.entries(internal.c).map(([name, ref]) => ({ name, ref }));
      case "d":
        return internal.c.map((ref, index) => ({ name: String(index), ref }));
      case "h":
        return Array.from({ length: internal.l }, (_unused, index) => ({
          name: String(index),
          ref: internal.c,
        }));
      default:
        throw new Error("Unknown schema kind");
    }
  }

  private kindFor(def: CompactDef): SchemaKind {
    if (!def.i) {
      return "leaf";
    }
    switch (def.i.k) {
      case "n":
        return "named";
      case "d":
        return "numbered";
      case "h":
        return "homogeneous";
      default:
        throw new Error("Unknown schema kind");
    }
  }
}

export function subtreeMatch(path: string, root: string): boolean {
  const normalizedRoot = validatePath(root);
  return (
    !normalizedRoot ||
    path === normalizedRoot ||
    path.startsWith(`${normalizedRoot}/`)
  );
}

export function displayPath(path: string): string {
  return path || "/";
}

function segment(path: string): string {
  return path ? (path.split("/").at(-1) ?? "") : "";
}

function formatScalar(value: unknown, quoteStrings = false): string {
  if (typeof value === "string") {
    return quoteStrings ? JSON.stringify(value) : value;
  }
  return JSON.stringify(value);
}

function formatMapping(prefix: string, value: unknown, quoteStrings = false): string {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return `${prefix} ${formatScalar(value, quoteStrings)}`;
  }
  const items = Object.entries(value)
    .map(([key, item]) =>
      item === true ? key : `${key}=${formatScalar(item, quoteStrings)}`,
    );
  return items.length ? `${prefix} ${items.join(" ")}` : prefix;
}

function formatMetadataValue(value: unknown): string {
  if (typeof value === "string") {
    return value;
  }
  return JSON.stringify(value) ?? String(value);
}

function metadataLines(prefix: string, value: unknown): string[] {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return [`${prefix} ${formatMetadataValue(value)}`];
  }
  const lines: string[] = [];
  for (const [key, item] of Object.entries(value)) {
    const text = formatMetadataValue(item);
    if (text.includes("\n")) {
      lines.push(`${prefix} ${key}:`);
      lines.push(...text.split(/\r?\n/).map((line) => `  ${line}`));
    } else {
      lines.push(item === true ? `${prefix} ${key}` : `${prefix} ${key}=${text}`);
    }
  }
  return lines.length ? lines : [prefix];
}

function annotations(node: SchemaNode, compressedHomogeneous = false): string[] {
  const tags: string[] = [];
  if (compressedHomogeneous || node.kind === "homogeneous") {
    tags.push("homogeneous");
  } else if (node.kind === "numbered") {
    tags.push("numbered");
  }
  if (node.sem !== undefined) {
    tags.push(formatMapping("sem", node.sem));
  }
  if (node.edge !== undefined) {
    tags.push(formatMapping("edge", node.edge, true));
  }
  if (node.node !== undefined) {
    tags.push(formatMapping("node", node.node, true));
  }
  return tags.map((tag) => `[${tag}]`);
}

export function formatSchemaName(node: SchemaNode, name?: string): string {
  return name ?? segment(node.path);
}

export function formatSchemaMetadata(node: SchemaNode): string {
  return [
    ...metadataLines("kind", node.kind),
    ...(node.sem === undefined ? [] : metadataLines("sem", node.sem)),
    ...(node.edge === undefined ? [] : metadataLines("edge", node.edge)),
    ...(node.node === undefined ? [] : metadataLines("node", node.node)),
  ].join("\n");
}

export function formatSchemaLabel(
  node: SchemaNode,
  options: { name?: string; compressedHomogeneous?: boolean } = {},
): string {
  const label = formatSchemaName(node, options.name);
  return [
    label,
    ...annotations(node, options.compressedHomogeneous ?? false),
  ].filter(Boolean).join(" ");
}

function treeLines(
  rootLine: string | undefined,
  children: [string, () => string[]][],
): string[] {
  const lines = rootLine === undefined ? [] : [rootLine];
  children.forEach(([label, descend], index) => {
    const last = index + 1 === children.length;
    lines.push(`${last ? "└─ " : "├─ "}${label}`);
    const prefix = last ? "   " : "│  ";
    for (const line of descend()) {
      lines.push(`${prefix}${line}`);
    }
  });
  return lines;
}

export function renderSchemaTree(schema: Schema, root = ""): string {
  const normalizedRoot = schema.path(root);

  function visit(path: string, compress: boolean): string[] {
    const node = schema.node(path);
    if (compress && node.kind === "homogeneous") {
      const children = schema.children(path);
      if (children.length) {
        const child = children[0];
        return treeLines(formatSchemaLabel(node), [
          [
            formatSchemaLabel(child, {
              name: `0..${node.children.length}`,
              compressedHomogeneous: true,
            }),
            () => visit(child.path, false).slice(1),
          ],
        ]);
      }
    }

    return treeLines(
      formatSchemaLabel(node),
      schema.children(path).map((child) => [
        formatSchemaLabel(child),
        () => visit(child.path, true).slice(1),
      ]),
    );
  }

  if (!normalizedRoot) {
    const children = schema.children("");
    const lines: string[] = [];
    children.forEach((child, index) => {
      const childLines = visit(child.path, true);
      const last = index + 1 === children.length;
      lines.push(`${last ? "└─ " : "├─ "}${childLines[0]}`);
      const prefix = last ? "   " : "│  ";
      for (const line of childLines.slice(1)) {
        lines.push(`${prefix}${line}`);
      }
    });
    return lines.join("\n");
  }
  return visit(normalizedRoot, true).join("\n");
}
