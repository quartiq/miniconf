"""Follow Miniconf definitions and edges without expanding the schema graph."""

from dataclasses import dataclass, replace


@dataclass
class Entry:
    path: str
    owner: str | None
    schema: dict
    defaults: dict
    edge: dict
    definition: str | None = None
    nullable: bool = False
    template: str | None = None

    @property
    def kind(self):
        return self.schema["tree-kind"]

    def metadata(self, scope: str) -> dict:
        return self.edge if scope == "edge" else self.schema.get("tree-node-meta", {})

    def addresses(self, scope: str) -> dict:
        meta = self.metadata(scope)
        result = {key: meta[key] for key in ("id", "typename") if key in meta}
        if scope == "edge" and "typename" not in result and self.owner:
            result["typename"] = self.owner
        return result


class Inventory:
    """One application, with homogeneous elements addressed by `{index}`.

    Input is TreeJsonSchema before AllowAbsent. Definitions stay shared;
    defaults keep concrete instance paths. Serialized leaf values are atomic.
    """

    def __init__(self, document: dict):
        if not isinstance(document, dict):
            raise ValueError("Miniconf schema document must be a JSON object")
        if not isinstance(document.get("$defs", {}), dict):
            raise ValueError("Schema $defs must be an object")
        if "title" in document and not isinstance(document["title"], str):
            raise ValueError("Schema title must be a string")
        self.document = document
        self.entries: dict[str, Entry] = {}
        defaults = {"": document["default"]} if "default" in document else {}
        self._visit(document, "", None, defaults)

    def value_schema(self, schema, refs=()):
        """Resolve serialized leaf representations, never Miniconf children."""
        if not isinstance(schema, dict):
            if isinstance(schema, bool):
                return schema
            raise ValueError("Value schema must be an object or boolean")
        if "$ref" in schema:
            ref = schema["$ref"]
            if ref in refs:
                raise ValueError(f"Unsupported or recursive schema reference: {ref}")
            schema = self.value_schema(self.resolve(ref), (*refs, ref)) | {
                k: v for k, v in schema.items() if k != "$ref"
            }
        for key in ("description", "format"):
            if key in schema and not isinstance(schema[key], str):
                raise ValueError(f"Value schema {key} must be a string")
        for key in ("required", "enum"):
            if key in schema and not isinstance(schema[key], list):
                raise ValueError(f"Value schema {key} must be an array")
        kind = schema.get("type", "JSON value")
        if not isinstance(kind, str) and not (
            isinstance(kind, list) and all(isinstance(item, str) for item in kind)
        ):
            raise ValueError("Value schema type must be a string or array of strings")
        result = {}
        for key, value in schema.items():
            if key in ("$defs", "$schema", "default"):
                continue
            if key == "properties":
                if not isinstance(value, dict):
                    raise ValueError("Value schema properties must be an object")
                value = {
                    name: self.value_schema(child, refs)
                    for name, child in value.items()
                }
            elif key in ("items", "additionalProperties"):
                value = self.value_schema(value, refs)
            elif key in ("prefixItems", "oneOf", "anyOf"):
                if not isinstance(value, list):
                    raise ValueError(f"Value schema {key} must be an array")
                value = [self.value_schema(child, refs) for child in value]
            result[key] = value
        return result

    def resolve(self, ref):
        if not isinstance(ref, str) or not ref.startswith("#/$defs/"):
            raise ValueError(f"Unsupported schema reference: {ref}")
        name = ref[8:].replace("~1", "/").replace("~0", "~")
        definition = self.document.get("$defs", {}).get(name)
        if not isinstance(definition, dict):
            raise ValueError(f"Missing or invalid schema definition: {ref}")
        return definition

    def context(self, entry: Entry, scope: str) -> dict:
        """Source context for an explanation, including immediate child facts."""
        result = self.local_context(entry)
        if entry.kind != "leaf":
            result["children"] = {}
            for child in self.children(entry.template or entry.path):
                name = child.path.rsplit("/", 1)[-1]
                selected = (
                    self.select(entry.path + "/" + name)[0] if entry.template else child
                )
                result["children"][name] = self.local_context(selected)
        if scope == "node":
            result["schema"].pop("tree-edge-meta", None)
            result.pop("nullable", None)
            for context in [result, *result.get("children", {}).values()]:
                context.pop("defaults", None)
        return result

    def children(self, path):
        return [
            entry
            for key, entry in self.entries.items()
            if key and key.rsplit("/", 1)[0] == path
        ]

    def local_context(self, entry):
        schema = dict(entry.schema)
        schema["tree-edge-meta"] = entry.edge
        for key in ("$schema", "$defs", "default", "title"):
            schema.pop(key, None)
        if entry.kind != "leaf":
            for key in ("properties", "items", "prefixItems", "oneOf", "anyOf"):
                schema.pop(key, None)
            schema["child order"] = [
                child.path.rsplit("/", 1)[-1]
                for child in self.children(entry.template or entry.path)
            ]
        defaults = {}

        def instance_key(path):
            return "/".join(
                part
                for segment, part in zip(
                    (entry.template or entry.path).split("/"), path.split("/")
                )
                if segment == "{index}"
            )

        for path, value in entry.defaults.items():
            # Internal defaults describe presence and active children; leaf values
            # are compared at their own paths to avoid unrelated review noise.
            if entry.kind != "leaf":
                if isinstance(value, dict):
                    value = sorted(value)
                elif isinstance(value, list):
                    value = {"length": len(value)}
            defaults[instance_key(path)] = value
        return {
            "schema": schema,
            "nullable": entry.nullable,
            "defaults": defaults,
        }

    def _visit(self, use, path, owner, defaults, ancestors=()):
        location = path or "(root)"
        if not isinstance(use, dict):
            raise ValueError(f"{location}: Miniconf node must be an object")
        nullable = "tree-kind" not in use and "anyOf" in use
        if nullable and not (
            isinstance(use["anyOf"], list)
            and len(use["anyOf"]) == 2
            and isinstance(use["anyOf"][0], dict)
            and use["anyOf"][1] in ({"type": "null"}, {"const": None})
        ):
            raise ValueError(f"{location}: expected anyOf: [node, {{type: null}}]")
        schema = use["anyOf"][0] if nullable else use
        reference = schema.get("$ref")
        if reference:
            if reference in ancestors:
                raise ValueError(f"Recursive Miniconf schema: {reference}")
            schema = self.resolve(reference)
        kind = schema.get("tree-kind")
        if kind not in ("leaf", "named", "numbered", "homogeneous"):
            raise ValueError(f"{location}: unknown Miniconf tree-kind {kind!r}")
        if "description" in schema and not isinstance(schema["description"], str):
            raise ValueError(f"{location}: description must be a string")
        for scope, meta in (
            ("node", schema.get("tree-node-meta", {})),
            ("edge", use.get("tree-edge-meta", {})),
        ):
            if not isinstance(meta, dict) or any(
                key in meta and not isinstance(meta[key], str)
                for key in ("id", "typename", "doc")
            ):
                raise ValueError(f"{location}: invalid {scope} metadata")
        for flag in ("tree-oneof", "tree-maybe-absent"):
            if flag in schema and not isinstance(schema[flag], bool):
                raise ValueError(f"{location}: {flag} must be boolean")
        if kind == "leaf":
            try:
                schema = self.value_schema(schema)
            except ValueError as error:
                raise ValueError(f"{location}: {error}") from error
        elif kind == "named":
            if not isinstance(schema.get("properties", {}), dict):
                raise ValueError(f"{location}: named properties must be an object")
        elif kind == "numbered":
            if not isinstance(schema.get("prefixItems"), list):
                raise ValueError(f"{location}: numbered prefixItems must be an array")
        elif (
            type(schema.get("maxItems")) is not int
            or schema["maxItems"] < 0
            or schema.get("minItems", schema["maxItems"]) != schema["maxItems"]
            or not isinstance(schema.get("items"), dict)
        ):
            raise ValueError(
                f"{location}: homogeneous node needs a fixed length and items"
            )
        if kind != "leaf":
            for base, value in defaults.items():
                if value is None or value in ("__tree-absent__", "__tree-access__"):
                    continue
                if kind == "named":
                    valid = isinstance(value, dict)
                else:
                    length = (
                        len(schema["prefixItems"])
                        if kind == "numbered"
                        else schema["maxItems"]
                    )
                    valid = isinstance(value, list) and len(value) == length
                if not valid:
                    raise ValueError(
                        f"{base or '(root)'}: default does not match {kind} node"
                    )
        entry = Entry(
            path,
            owner,
            schema,
            defaults,
            use.get("tree-edge-meta", {}),
            reference,
            nullable,
        )
        self.entries[path] = entry
        if kind == "leaf":
            return
        typename = schema.get("tree-node-meta", {}).get("typename")
        children = schema.get("properties", {})
        if kind == "numbered":
            children = dict(enumerate(schema["prefixItems"]))
        elif kind == "homogeneous":
            children = {"{index}": schema["items"]}
        for name, child in children.items():
            values = {}
            for base, value in defaults.items():
                if kind == "homogeneous" and isinstance(value, list):
                    values.update((f"{base}/{i}", v) for i, v in enumerate(value))
                elif isinstance(value, dict):
                    if name in value or not schema.get("tree-oneof"):
                        values[f"{base}/{name}"] = value.get(name, "__tree-absent__")
                elif kind == "numbered" and isinstance(value, list):
                    values[f"{base}/{name}"] = value[name]
            self._visit(
                child,
                f"{path}/{name}",
                f"{typename}.{name}" if typename and kind == "named" else None,
                values,
                (*ancestors, reference),
            )

    def select(self, path):
        """Resolve documentation paths, including concrete homogeneous indices."""

        def visit(entry, parts, actual):
            if not parts:
                if actual == entry.path:
                    return [entry]
                defaults = {
                    key: value
                    for key, value in entry.defaults.items()
                    if all(
                        a == "{index}" or a == b
                        for a, b in zip(actual.split("/"), key.split("/"))
                    )
                }
                return [
                    replace(entry, path=actual, defaults=defaults, template=entry.path)
                ]
            part, *tail = parts
            children = self.children(entry.path)
            if entry.kind == "homogeneous" and part != "+" and part != "{index}":
                if (
                    not part.isdecimal()
                    or str(int(part)) != part
                    or int(part) >= entry.schema["maxItems"]
                ):
                    return []
                return visit(children[0], tail, actual + "/" + part)
            result = []
            for child in children:
                name = child.path.rsplit("/", 1)[-1]
                if part == "+" or part == name:
                    result.extend(visit(child, tail, actual + "/" + name))
            return result

        return visit(self.entries[""], path[1:].split("/") if path else [], "")


def default_rows(inventory, entry, value, path=""):
    """Walk Miniconf children, stopping at leaves even for structured JSON values."""
    if isinstance(value, str) and value in ("__tree-absent__", "__tree-access__"):
        yield (
            path,
            entry,
            None,
            "absent" if value == "__tree-absent__" else "access denied",
        )
        return
    if entry.kind == "leaf":
        yield path, entry, value, None
        return
    if value is None:
        yield path, entry, None, "null subtree"
        return
    children = inventory.children(entry.path)
    missing = object()
    if entry.kind == "homogeneous":
        items = [(str(index), children[0], item) for index, item in enumerate(value)]
    else:
        items = []
        for child in children:
            name = child.path.rsplit("/", 1)[-1]
            item = (
                value.get(name, missing)
                if isinstance(value, dict)
                else (value[int(name)] if int(name) < len(value) else missing)
            )
            items.append((name, child, item))
    if not children or (entry.kind == "homogeneous" and not value):
        yield path, entry, None, "empty subtree"
    for name, child, item in items:
        child_path = f"{path}/{name}" if path else name
        if item is missing:
            if not entry.schema.get("tree-oneof"):
                yield child_path, child, None, "absent"
            continue
        if entry.schema.get("tree-oneof"):
            yield child_path, child, None, "selected variant"
            if (
                child.kind != "leaf"
                and not inventory.children(child.path)
                and item == {}
            ):
                continue
        yield from default_rows(inventory, child, item, child_path)
