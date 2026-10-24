"""Choose shared reference sections and the uses that own instance defaults."""


def sections(inventory, attachments):
    groups, by_path = {}, {}
    for entry in inventory.entries.values():
        if entry.path and not (
            entry.metadata("node").get("typename")
            or entry.metadata("node").get("doc")
            or entry.schema.get("description")
            or (entry.path, "node") in attachments
        ):
            continue
        if (
            entry.path
            and entry.kind == "leaf"
            and not (
                entry.metadata("node").get("doc")
                or entry.schema.get("description")
                or (entry.path, "node") in attachments
            )
        ):
            continue
        selected = tuple(
            (path[len(entry.path) :], scope, doc.topic)
            for (path, scope), doc in attachments.items()
            if (path == entry.path and scope == "node")
            or path.startswith(entry.path + "/")
        )
        # The exporter establishes definition equivalence, including metadata.
        key = (entry.definition or entry.path, selected)
        group = groups.setdefault(key, [])
        group.append(entry)
        by_path[entry.path] = group
    # Count locations in the composed reference, not expanded instances of a
    # shared parent. A child used only inside that parent belongs inline there.
    locations, uses = {}, {}
    for path in inventory.entries:
        parent, _, name = path.rpartition("/")
        location = locations[parent] + "/" + name if path else ""
        group = by_path.get(path)
        if group:
            representative = group[0].path
            uses.setdefault(representative, set()).add(location)
            location = locations.get(representative, location)
        locations[path] = location
    return {
        path: group
        for path, group in by_path.items()
        if not path or len(uses[group[0].path]) > 1
    }


def default_owners(inventory, sections):
    owners = set()
    for entry in inventory.entries.values():
        if any(entry.path.startswith(parent + "/") for parent in owners):
            continue
        if (
            entry.kind == "leaf"
            or (entry.path and entry.path in sections)
            or entry.schema.get("tree-oneof")
            or any(
                value is None or value in ("__tree-absent__", "__tree-access__")
                for value in entry.defaults.values()
            )
        ):
            owners.add(entry.path)
    return owners
