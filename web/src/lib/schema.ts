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
    const normalized = validatePath(path);
    const ref = this.resolve(normalized);
    return this.nodeFrom(normalized, ref, this.childEntries(refId(ref)));
  }

  private nodeFrom(
    path: string,
    ref: CompactRef,
    entries: { name: string; ref: CompactRef }[],
  ): SchemaNode {
    const id = refId(ref);
    const def = this.defs[id];
    const children = entries.map(({ name, ref }) => ({
      name,
      path: `${path}/${name}`,
      ...(refMeta(ref) === undefined ? {} : { edge: refMeta(ref) }),
    }));
    return {
      path,
      kind: this.kindFor(def),
      ...(def.m === undefined ? {} : { node: def.m }),
      ...(refMeta(ref) === undefined ? {} : { edge: refMeta(ref) }),
      ...(def.s === undefined ? {} : { sem: def.s }),
      children,
    };
  }

  children(path = ""): SchemaNode[] {
    return this.node(path).children.map((child) => this.node(child.path));
  }

  walk(path = ""): SchemaNode[] {
    const nodes: SchemaNode[] = [];
    const visit = (path: string, ref: CompactRef) => {
      const id = refId(ref);
      if (id < 0 || id >= this.defs.length) {
        throw new Error(`Invalid schema reference ${id} in ${path}`);
      }
      const entries = this.childEntries(id);
      const node = this.nodeFrom(path, ref, entries);
      nodes.push(node);
      entries.forEach(({ ref }, index) =>
        visit(node.children[index].path, ref),
      );
    };
    visit(validatePath(path), this.resolve(path));
    return nodes;
  }

  private resolve(path: string): CompactRef {
    let id = this.root;
    let ref: CompactRef = id;
    if (!path) return ref;
    for (const part of path.slice(1).split("/")) {
      const internal = this.defs[id].i;
      let child: CompactRef | undefined;
      if (internal?.k === "n") {
        if (Object.hasOwn(internal.c, part)) child = internal.c[part];
      } else if (internal) {
        if (internal.k !== "d" && internal.k !== "h") {
          throw new Error("Unknown schema kind");
        }
        const index = Number(part);
        if (Number.isInteger(index) && index >= 0 && String(index) === part) {
          if (internal.k === "d") child = internal.c[index];
          else if (internal.k === "h" && index < internal.l) child = internal.c;
        }
      }
      if (child === undefined) {
        throw new Error(`Unknown schema path: ${path}`);
      }
      ref = child;
      id = refId(ref);
      if (id < 0 || id >= this.defs.length) {
        throw new Error(`Invalid schema reference ${id} in ${path}`);
      }
    }
    return ref;
  }

  private childEntries(id: number): { name: string; ref: CompactRef }[] {
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
  return path || "(root)";
}

export function formatSchemaName(node: SchemaNode): string {
  return node.path.split("/").at(-1) || '\"\"';
}

export function schemaSummary(node: SchemaNode): string {
  const parts: string[] = node.kind === "leaf" ? [] : [node.kind];
  // Only Sem fields defined by Rust carry portable meaning. Other metadata is opaque.
  if (node.sem && typeof node.sem === "object" && !Array.isArray(node.sem)) {
    const sem = node.sem as Record<string, unknown>;
    if (
      typeof sem.ty === "string" &&
      /^(bool|[iu](8|16|32|64|128|size)|f(32|64)|str)$/.test(sem.ty)
    )
      parts.push(sem.ty);
    if (sem.oneof === true) parts.push("mutually exclusive children");
    if (sem.maybe_absent === true) parts.push("may be absent");
  }
  return parts.join(" · ");
}

export function schemaTooltip(node: SchemaNode): string {
  const sections: [string, unknown][] = [
    ["Semantics", node.sem],
    ["Edge metadata", node.edge],
    ["Node metadata", node.node],
  ];
  return [
    displayPath(node.path),
    schemaSummary(node),
    ...sections.flatMap(([label, value]) => {
      if (value === undefined) return [];
      const entries =
        value !== null &&
        typeof value === "object" &&
        !Array.isArray(value) &&
        Object.keys(value).length
          ? Object.entries(value)
          : [[null, value]];
      return [
        `${label}:\n${entries
          .map(
            ([key, item]) =>
              `${key === null ? "" : `${key || '""'}: `}${typeof item === "string" ? item : JSON.stringify(item)}`,
          )
          .join("\n")}`,
      ];
    }),
  ].join("\n\n");
}
