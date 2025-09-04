#![cfg_attr(not(any(test, feature = "std")), no_std)]
#![cfg_attr(all(feature = "derive", feature = "json-core"), doc = include_str!("../README.md"))]
#![cfg_attr(not(all(feature = "derive", feature = "json-core")), doc = "Miniconf")]
#![deny(rust_2018_compatibility)]
#![deny(rust_2018_idioms)]
// #![warn(missing_docs)]
#![forbid(unsafe_code)]

/// Traversal, iteration of keys in a tree.
///
/// See also the sub-traits [`TreeSerialize`], [`TreeDeserialize`], [`TreeAny`].
///
/// # Keys
///
/// There is a one-to-one relationship between nodes and keys.
/// The keys used to identify nodes support [`Keys`]/[`IntoKeys`]. They can be
/// obtained from other [`IntoKeys`] through [`Transcode`]/[`TreeSchema::transcode()`].
/// An iterator of keys for the nodes is available through [`TreeSchema::nodes()`]/[`NodeIter`].
///
/// * `usize` is modelled after ASN.1 Object Identifiers, see [`crate::Indices`].
/// * `&str` keys are sequences of names, like path names. When concatenated, they are separated
///   by some path hierarchy separator, e.g. `'/'`, see [`crate::Path`], or by some more
///   complex notation, see [`crate::JsonPath`].
/// * [`crate::Packed`] is a bit-packed compact compressed notation of
///   hierarchical compound indices.
/// * See the `scpi` example for how to implement case-insensitive, relative, and abbreviated/partial
///   matches.
///
/// # Derive macros
///
/// Derive macros to automatically implement the correct traits on a struct or enum are available through
/// [`macro@crate::TreeSchema`], [`macro@crate::TreeSerialize`], [`macro@crate::TreeDeserialize`],
/// and [`macro@crate::TreeAny`].
/// A shorthand derive macro that derives all four trait implementations is also available at
/// [`macro@crate::Tree`].
///
/// The derive macros support per-field/per-variant attributes to control the derived trait implementations.
///
/// ## Rename
///
/// The key for named struct fields or enum variants may be changed from the default field ident using
/// the `rename` derive macro attribute.
///
/// ```
/// use miniconf::{Leaf, Path, Tree, TreeSchema};
/// #[derive(Tree, Default)]
/// struct S {
///     #[tree(rename = "OTHER")]
///     a: Leaf<f32>,
/// };
/// let (name, _node) = S::transcode::<Path<String, '/'>, _>([0usize]).unwrap();
/// assert_eq!(name.as_str(), "/OTHER");
/// ```
///
/// ## Skip
///
/// Named fields/variants may be omitted from the derived `Tree` trait implementations using the
/// `skip` attribute.
/// Note that for tuple structs skipping is only supported for terminal fields:
///
/// ```
/// use miniconf::{Leaf, Tree};
/// #[derive(Tree)]
/// struct S(Leaf<i32>, #[tree(skip)] ());
/// ```
///
/// ```compile_fail
/// use miniconf::{Tree, Leaf};
/// #[derive(Tree)]
/// struct S(#[tree(skip)] (), Leaf<i32>);
/// ```
///
/// ## Type
///
/// The type to use when accessing the field/variant through `TreeSchema`/`TreeDeserialize::probe`
/// can be overridden using the `typ` derive macro attribute (`#[tree(typ="[f32; 4]")]`).
///
/// ## Deny
///
/// `#[tree(deny(operation="message", ...))]`
///
/// This returns `Err(`[`Traversal::Access`]`)` for the respective operation
/// (`traverse`, `serialize`, `deserialize`, `probe`, `ref_any`, `mut_any`) on a
/// field/variant and suppresses the respective traits bounds on type paramters
/// of the struct/enum.
///
/// ## Implementation overrides
///
/// `#[tree(with(operation=expr, ...))]`
///
/// This overrides the call to the child node/variant trait for the given `operation`
/// (`traverse`, `traverse_all`, `serialize`, `deserialize`, `probe`, `ref_any`, `mut_any`).
/// `expr` should be a method on `self` (not the field!) or `value`
/// (associated function for `traverse`, `traverse_all` and `probe`)
/// taking the arguments of the respective trait's method.
///
/// ```
/// # use miniconf::{Error, Leaf, Tree, Keys, Traversal, TreeDeserialize};
/// # use serde::Deserializer;
/// #[derive(Tree, Default)]
/// struct S {
///     #[tree(with(deserialize=self.check))]
///     b: Leaf<f32>,
/// };
/// impl S {
///     fn check<'de, K: Keys, D: Deserializer<'de>>(&mut self, keys: K, de: D) -> Result<(), Error<D::Error>> {
///         let old = *self.b;
///         self.b.deserialize_by_key(keys, de)?;
///         if *self.b < 0.0 {
///             *self.b = old;
///             Err(Traversal::Access(0, "fail").into())
///         } else {
///             Ok(())
///         }
///     }
/// }
/// ```
///
/// ### `defer`
///
/// The `defer` attribute is a shorthand for `with()` that defers
/// child trait implementations to a given expression.
///
/// # Array
///
/// Blanket implementations of the `Tree*` traits are provided for homogeneous arrays
/// [`[T; N]`](core::array).
///
/// # Option
///
/// Blanket implementations of the `Tree*` traits are provided for [`Option<T>`].
///
/// These implementations do not alter the path hierarchy and do not consume any items from the `keys`
/// iterators. The `TreeSchema` behavior of an [`Option`] is such that the `None` variant makes the
/// corresponding part of the tree inaccessible at run-time. It will still be iterated over (e.g.
/// by [`TreeSchema::nodes()`]) but attempts to access it (e.g. [`TreeSerialize::serialize_by_key()`],
/// [`TreeDeserialize::deserialize_by_key()`], [`TreeAny::ref_any_by_key()`], or
/// [`TreeAny::mut_any_by_key()`]) return the special [`Traversal::Absent`].
///
/// This is the same behavior as for other `enums` that have the `Tree*` traits derived.
///
/// # Tuples
///
/// Blanket impementations for the `Tree*` traits are provided for heterogeneous tuples `(T0, T1, ...)`
/// up to length eight.
///
/// # Examples
///
/// See the [`crate`] documentation for a longer example showing how the traits and the derive
/// macros work.
mod error;
pub use error::*;
mod key;
pub use key::*;
mod key_impls;
pub use key_impls::*;
mod schema;
pub use schema::*;
mod shape;
pub use shape::*;
mod packed;
pub use packed::*;
mod jsonpath;
pub use jsonpath::*;
mod tree;
pub use tree::*;
mod iter;
pub use iter::*;
mod impls;
mod leaf;
pub use leaf::*;

#[cfg(feature = "derive")]
pub use miniconf_derive::*;

#[cfg(feature = "json-core")]
pub mod json;

#[cfg(feature = "postcard")]
pub mod postcard;

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "trace")]
pub mod trace;

#[cfg(feature = "schema")]
pub mod json_schema;

// re-export for proc-macro
#[doc(hidden)]
pub use serde::{Deserialize, Deserializer, Serialize, Serializer};
