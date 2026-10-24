# Miniconf documentation

Generate a settings reference from a Miniconf JSON Schema export and Markdown
explanations. Schema descriptions and metadata supply the API reference; authored
prose adds context. References start at the root; fields supply their context and
instance defaults.

Requires Python 3.11 or newer. Install with `pip install .`.

## Example

The [example schema](examples/schema.json) uses the same `Channel` for an output
and a monitor, with different defaults. Its [explanations](examples/docs/channel.md)
share one Markdown file:

````markdown
```miniconf
scope = "node"
typename = "Channel"
```

# Channel operation

Channels share the acquisition clock. Changes take effect at the next frame.

```miniconf
scope = "edge"
typename = "Channel.gain"
```

# Gain calibration

Gain converts the channel's input voltage to output current, in A/V.
````

Build the reference:

```sh
miniconf_doc build --output /tmp/reference.md examples/schema.json examples/docs
```

Pass additional directories to combine shared prose with application-specific
explanations: `miniconf_doc build schema.json docs/common docs/instrument`.
Directories are searched recursively and selected explicitly for each schema.
Each build uses one schema, independently of its filename or application name.

## Writing explanations

Each top-level `miniconf` fence contains a TOML selector. Its explanation runs to
the next selector or the end of the file. Use ordinary Markdown with distinct
headings in each file. Put prose after its selector. Files without selectors can
serve as ordinary documentation pages.

`scope = "node"` explains a reusable node. `scope = "edge"` explains its use in
its parent. Keep units, wiring, and application-specific behavior on the edge;
shared configuration semantics belong to the node.

Selectors match in this order: `id`, `typename`, `path`. Specify one
selector per explanation. `id` uses existing metadata when available; `typename`
uses node or edge metadata, with `ParentType.field` as an edge fallback. `path`
selects a settings path (`""` for the root) and supports MQTT-style `+` for one
segment, for example `path = "/channel/+/controller"`. Exact paths take precedence
over wildcard paths; overlapping wildcard matches are errors. Only `+` is supported.
Concrete array indices select an instance; the reference uses `{index}` for the
shared element schema. Directory order gives no precedence.

Use `file.md#heading` and `#heading` links between explanations. Reference labels
are local to each explanation. Links to other pages and assets are resolved from
the source file. Rust API links need explicit URLs.

## Reviewing changes

```sh
miniconf_doc check schema.json docs
miniconf_doc diff old-schema.json new-schema.json docs
```

`check` validates selectors and composition, including unmatched explanations.
`build` reports unmatched explanations but permits partial previews and prose
pools shared with other schemata. `diff` reports changed settings and identifies
affected explanations by source file and line. Generate each export from its
corresponding source revision. Type selectors follow path moves; renamed types
and fields require selector updates. Runtime behavior changes also need prose
review, even when the schema stays the same.

## Publishing

The generated file uses Markdown headings, paragraphs, lists, tables, and code
blocks. Small HTML anchors provide stable link targets. It works with mdBook and
other Markdown renderers that support HTML anchors. Page links preserve `.md`;
use `build --html-links` when publishing them as `.html`.
`python -m miniconf_doc.links HTML_DIRECTORY` checks
links and anchors in that directory's HTML pages.

For custom consumers, `Inventory` and `Documents.attach()` expose resolved schema
and explanations independently of rendering. `render(..., page_link=...)` accepts
a function mapping external Markdown destination paths; assets and composed
explanation links keep their own destinations.

Keep authored Markdown, schemas' source code, and build configuration in version
control. Generate reference pages and schema exports during the documentation
build. Long-form prose stays on the host; firmware carries only its schema data.

Input follows Miniconf 0.22's `TreeJsonSchema` projection before `AllowAbsent`,
with local, acyclic references. The schema document is a JSON object; the root
setting can be any JSON value. An optional root `default` uses Miniconf's
diagnostic value encoding, including its reserved absence/access markers.

Leaf descriptions summarize value shapes and selected constraints; publish the
original export alongside the reference for complete constraints. `check`
validates tree structure and explanation heading links as well as selectors.
Cross-page links and assets require the post-render HTML check above.
