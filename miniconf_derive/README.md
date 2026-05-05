# `miniconf` Derive Macros

This package contains derive macros for [`miniconf`](https://crates.io/crates/miniconf).

Most users import these macros through `miniconf` and derive `Tree` there. Attribute syntax and
derive behavior are documented in this crate's rustdoc.

## Limitations

- The derives cover a restricted tree model. Enums with named fields or multi-field tuple variants are not supported as internal tree nodes.
- Flattening is only supported where the generated lookup is unambiguous.
- Diagnostics are compile-time oriented and sometimes point at generated trait use rather than the original high-level intent. Check the expanded field or variant shape first.
