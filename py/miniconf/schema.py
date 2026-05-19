"""Schema tree and key translation for Miniconf MQTT."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Iterator

from .common import MiniconfException, validate_path


def _segment(path: str) -> str:
    return path.rsplit("/", 1)[-1]


def _bits_for(num: int) -> int:
    return max(1, num.bit_length())


def _ref_id(child: int | dict[str, Any]) -> int:
    return child if isinstance(child, int) else child["r"]


def _ref_meta(child: int | dict[str, Any]) -> Any:
    return None if isinstance(child, int) else child.get("m")


@dataclass(frozen=True)
class Indices:
    value: tuple[int, ...]

    def __iter__(self):
        return iter(self.value)

    def __len__(self) -> int:
        return len(self.value)


@dataclass(frozen=True)
class Packed:
    value: int

    def __int__(self) -> int:
        return self.value


@dataclass(frozen=True)
class SchemaNode:
    path: str
    schema: dict[str, Any]

    @property
    def node(self) -> Any:
        return self.schema.get("node")

    @property
    def edge(self) -> Any:
        return self.schema.get("edge")

    @property
    def kind(self) -> str:
        return self.schema.get("internal", {}).get("kind", "leaf")


Keys = str | Indices | Packed | tuple[str | int, ...] | list[str | int]


class Schema:
    """Loaded compact schema defs with key translation helpers."""

    def __init__(self, defs: list[dict[str, Any]], rev: int):
        self.rev = rev
        self._defs = defs
        self._root = len(defs) - 1

    @classmethod
    def from_defs(cls, defs: list[dict[str, Any]], rev: int) -> Schema:
        return cls(defs, rev)

    def __iter__(self) -> Iterator[SchemaNode]:
        return self.walk()

    def __len__(self) -> int:
        return sum(1 for _ in self.walk())

    def _resolve(
        self, path: str
    ) -> tuple[
        int, tuple[int, ...], dict[str, Any] | None, int | dict[str, Any] | None
    ]:
        schema_id = self._root
        state: tuple[int, ...] = ()
        parent_internal = None
        child_ref = None
        if not path:
            return schema_id, state, parent_internal, child_ref
        for part in path.removeprefix("/").split("/"):
            parent_internal = self._defs[schema_id].get("i")
            if parent_internal is None:
                raise MiniconfException("NotFound", path)
            match parent_internal["k"]:
                case "n":
                    try:
                        names = list(parent_internal["c"].items())
                        index = next(
                            i for i, (name, _) in enumerate(names) if name == part
                        )
                        child_ref = names[index][1]
                    except StopIteration as exc:
                        raise MiniconfException("NotFound", path) from exc
                case "d":
                    try:
                        index = int(part)
                        child_ref = parent_internal["c"][index]
                    except (ValueError, IndexError) as exc:
                        raise MiniconfException("NotFound", path) from exc
                case "h":
                    try:
                        index = int(part)
                    except ValueError as exc:
                        raise MiniconfException("NotFound", path) from exc
                    if index < 0 or index >= parent_internal["l"]:
                        raise MiniconfException("NotFound", path)
                    child_ref = parent_internal["c"]
                case kind:
                    raise MiniconfException("Protocol", f"Unknown schema kind: {kind}")
            schema_id = _ref_id(child_ref)
            state = (*state, index)
        return schema_id, state, parent_internal, child_ref

    def _schema_def(self, path: str) -> dict[str, Any]:
        return self._defs[self._resolve(path)[0]]

    def _child_ref(self, path: str) -> int | dict[str, Any] | None:
        return self._resolve(path)[3]

    def _schema_view(self, path: str) -> dict[str, Any]:
        schema = self._schema_def(path)
        record: dict[str, Any] = {}
        internal = schema.get("i")
        if internal is not None:
            match internal["k"]:
                case "n":
                    record["internal"] = {
                        "kind": "named",
                        "children": [
                            {
                                "name": name,
                                **(
                                    {"edge": _ref_meta(child)}
                                    if _ref_meta(child) is not None
                                    else {}
                                ),
                            }
                            for name, child in internal["c"].items()
                        ],
                    }
                case "d":
                    record["internal"] = {
                        "kind": "numbered",
                        "children": [
                            (
                                {"edge": _ref_meta(child)}
                                if _ref_meta(child) is not None
                                else {}
                            )
                            for child in internal["c"]
                        ],
                    }
                case "h":
                    child = internal["c"]
                    record["internal"] = {
                        "kind": "homogeneous",
                        "child": (
                            {"edge": _ref_meta(child)}
                            if _ref_meta(child) is not None
                            else {}
                        ),
                        "len": internal["l"],
                    }
                case kind:
                    raise MiniconfException("Protocol", f"Unknown schema kind: {kind}")
        if "m" in schema:
            record["node"] = schema["m"]
        if "s" in schema:
            record["sem"] = schema["s"]
        child = self._child_ref(path)
        if child is not None and _ref_meta(child) is not None:
            record["edge"] = _ref_meta(child)
        return record

    def _child_paths(self, path: str) -> list[str]:
        internal = self._schema_def(path).get("i")
        if internal is None:
            return []
        match internal["k"]:
            case "n":
                names = internal["c"].keys()
            case "d":
                names = map(str, range(len(internal["c"])))
            case "h":
                names = map(str, range(internal["l"]))
            case kind:
                raise MiniconfException("Protocol", f"Unknown schema kind: {kind}")
        return [f"{path}/{name}" if path else f"/{name}" for name in names]

    def compact(self, keys: Keys = "") -> dict[str, Any]:
        root = self.path(keys)
        defs: list[dict[str, Any]] = []
        ids: dict[int, int] = {}

        def remap_ref(child: int | dict[str, Any]) -> int | dict[str, Any]:
            if isinstance(child, int):
                return remap(child)
            remapped = {"r": remap(child["r"])}
            if "m" in child:
                remapped["m"] = child["m"]
            return remapped

        def remap(schema_id: int) -> int:
            if schema_id in ids:
                return ids[schema_id]
            schema = self._defs[schema_id]
            internal = schema.get("i")
            if internal is not None:
                match internal["k"]:
                    case "n":
                        children = {
                            name: remap_ref(child)
                            for name, child in internal["c"].items()
                        }
                    case "d":
                        children = [remap_ref(child) for child in internal["c"]]
                    case "h":
                        children = remap_ref(internal["c"])
                    case _:
                        raise AssertionError("unreachable")
            local = len(defs)
            ids[schema_id] = local
            compact: dict[str, Any] = {}
            if "m" in schema:
                compact["m"] = schema["m"]
            if "s" in schema:
                compact["s"] = schema["s"]
            if internal is not None:
                node = {"k": internal["k"]}
                match internal["k"]:
                    case "n":
                        node["c"] = children
                    case "d":
                        node["c"] = children
                    case "h":
                        node["c"] = children
                        node["l"] = internal["l"]
                    case _:
                        raise AssertionError("unreachable")
                compact["i"] = node
            defs.append(compact)
            return local

        remap(self._resolve(root)[0])
        return {"path": root, "rev": self.rev, "defs": defs}

    def walk(self, keys: Keys = "") -> Iterator[SchemaNode]:
        def visit(path: str) -> Iterator[SchemaNode]:
            yield SchemaNode(path, self._schema_view(path))
            for child in self._child_paths(path):
                yield from visit(child)

        return visit(self.path(keys))

    def node(self, keys: Keys = "") -> SchemaNode:
        path = self.path(keys)
        return SchemaNode(path, self._schema_view(path))

    def children(self, keys: Keys = "") -> list[SchemaNode]:
        return [self.node(child) for child in self._child_paths(self.path(keys))]

    def path(self, keys: Keys = "") -> str:
        match keys:
            case str() as path:
                path = validate_path(path)
                self._resolve(path)
                return path
            case Packed(value=value):
                return self._path_from_packed(value)
            case Packed():
                raise AssertionError("unreachable")
            case Indices(value=value):
                return self._path_from_indices(value)
            case list() | tuple():
                if all(isinstance(part, int) for part in keys):
                    return self._path_from_indices(keys)
                return self._path_from_parts(keys)
            case _:
                raise TypeError(f"Unsupported keys: {keys!r}")

    def indices(self, keys: Keys = "") -> Indices:
        return Indices(self._resolve(self.path(keys))[1])

    def packed(self, keys: Keys = "") -> Packed:
        path = self.path(keys)
        value = 1
        parent = ""
        for index in self.indices(path):
            bits = _bits_for(len(self._child_paths(parent)) - 1)
            value = (value << bits) | index
            parent = self._child_paths(parent)[index]
        return Packed(value)

    def _path_from_parts(self, parts: tuple[str | int, ...] | list[str | int]) -> str:
        path = ""
        for part in parts:
            name = str(part)
            for child in self._child_paths(path):
                if _segment(child) == name:
                    path = child
                    break
            else:
                raise MiniconfException("NotFound", parts)
        return path

    def _path_from_indices(self, indices: tuple[int, ...] | list[int]) -> str:
        path = ""
        for index in indices:
            children = self._child_paths(path)
            try:
                path = children[index]
            except IndexError as exc:
                raise MiniconfException("NotFound", indices) from exc
        return path

    def _path_from_packed(self, value: int) -> str:
        if value <= 0:
            raise MiniconfException("NotFound", value)
        remaining = value.bit_length() - 1
        value ^= 1 << remaining
        path = ""
        while remaining:
            children = self._child_paths(path)
            bits = _bits_for(len(children) - 1)
            if remaining < bits:
                raise MiniconfException("NotFound", value)
            remaining -= bits
            index = value >> remaining
            value &= (1 << remaining) - 1
            try:
                path = children[index]
            except IndexError as exc:
                raise MiniconfException("NotFound", index) from exc
        return path
