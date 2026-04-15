use core::{convert::Infallible, num::NonZero};
#[cfg(feature = "meta-str")]
use serde::ser::SerializeMap;
use serde::{
    Serialize,
    ser::{SerializeSeq, Serializer},
};

use crate::{
    DescendError, ExactSize, FromConfig, IntoKeys, KeyError, Keys, NodeIter, Shape, Transcode,
};

#[cfg(feature = "sem")]
type MaybeSem = Option<Sem>;
#[cfg(not(feature = "sem"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
/// Empty structured semantics storage when `sem` is disabled.
pub struct MaybeSem;

#[cfg(feature = "sem")]
/// Empty structured semantics payload.
pub const NO_SEM: MaybeSem = None;
#[cfg(not(feature = "sem"))]
/// Empty structured semantics payload.
pub const NO_SEM: MaybeSem = MaybeSem;

#[cfg(feature = "sem")]
/// Structured semantics for a mutually exclusive named node.
pub const ONEOF_SEM: MaybeSem = Some(Sem {
    ty: None,
    oneof: true,
    maybe_absent: false,
});
#[cfg(feature = "sem")]
/// Structured semantics for a node that may be absent at runtime.
pub const MAYBE_ABSENT_SEM: MaybeSem = Some(Sem {
    ty: None,
    oneof: false,
    maybe_absent: true,
});
#[cfg(not(feature = "sem"))]
/// Structured semantics for a mutually exclusive named node.
pub const ONEOF_SEM: MaybeSem = MaybeSem;
#[cfg(not(feature = "sem"))]
/// Structured semantics for a node that may be absent at runtime.
pub const MAYBE_ABSENT_SEM: MaybeSem = MaybeSem;
/// Result of an exact key lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lookup {
    /// The number of keys consumed.
    pub depth: usize,
    /// The schema reached by the traversal.
    pub schema: &'static Schema,
}

/// Error returned by [`Schema::resolve_into()`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolveError {
    /// The traversal error.
    pub error: DescendError<()>,
    /// The number of keys consumed before the error.
    pub depth: usize,
    /// Whether the traversal had already reached a leaf when known.
    pub leaf: Option<bool>,
}

/// Structured machine-readable schema semantics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[non_exhaustive]
pub struct Sem {
    /// Semantic leaf type when known.
    #[cfg(feature = "sem")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ty: Option<Ty>,
    /// This named internal node has mutually exclusive children.
    #[cfg(feature = "sem")]
    #[serde(skip_serializing_if = "core::ops::Not::not")]
    pub oneof: bool,
    /// This node may be absent at runtime.
    #[cfg(feature = "sem")]
    #[serde(skip_serializing_if = "core::ops::Not::not")]
    pub maybe_absent: bool,
}

impl Sem {
    /// Semantic leaf type when known.
    pub const fn ty(&self) -> Option<Ty> {
        #[cfg(feature = "sem")]
        {
            self.ty
        }
        #[cfg(not(feature = "sem"))]
        {
            None
        }
    }

    /// Whether the node has mutually exclusive children.
    pub const fn oneof(&self) -> bool {
        #[cfg(feature = "sem")]
        {
            self.oneof
        }
        #[cfg(not(feature = "sem"))]
        {
            false
        }
    }

    /// Whether the node may be absent at runtime.
    pub const fn maybe_absent(&self) -> bool {
        #[cfg(feature = "sem")]
        {
            self.maybe_absent
        }
        #[cfg(not(feature = "sem"))]
        {
            false
        }
    }
}

/// Compact semantic leaf type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[non_exhaustive]
pub enum Ty {
    /// Boolean.
    #[serde(rename = "bool")]
    Bool,
    /// 8-bit signed integer.
    #[serde(rename = "i8")]
    I8,
    /// 16-bit signed integer.
    #[serde(rename = "i16")]
    I16,
    /// 32-bit signed integer.
    #[serde(rename = "i32")]
    I32,
    /// 64-bit signed integer.
    #[serde(rename = "i64")]
    I64,
    /// 128-bit signed integer.
    #[serde(rename = "i128")]
    I128,
    /// Pointer-sized signed integer.
    #[serde(rename = "isize")]
    Isize,
    /// 8-bit unsigned integer.
    #[serde(rename = "u8")]
    U8,
    /// 16-bit unsigned integer.
    #[serde(rename = "u16")]
    U16,
    /// 32-bit unsigned integer.
    #[serde(rename = "u32")]
    U32,
    /// 64-bit unsigned integer.
    #[serde(rename = "u64")]
    U64,
    /// 128-bit unsigned integer.
    #[serde(rename = "u128")]
    U128,
    /// Pointer-sized unsigned integer.
    #[serde(rename = "usize")]
    Usize,
    /// 32-bit floating point number.
    #[serde(rename = "f32")]
    F32,
    /// 64-bit floating point number.
    #[serde(rename = "f64")]
    F64,
    /// String-like leaf.
    #[serde(rename = "str")]
    Str,
}

/// Serializable view of metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MetaView<'a>(pub &'a Meta);

impl<'a> MetaView<'a> {
    fn new(meta: Option<&'a Meta>) -> Option<Self> {
        meta.map(Self)
    }
}

impl Serialize for MetaView<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[cfg(feature = "meta-str")]
        {
            let mut map = serializer.serialize_map(Some(self.0.len()))?;
            for (key, value) in *self.0 {
                map.serialize_entry(key, value)?;
            }
            map.end()
        }
        #[cfg(not(feature = "meta-str"))]
        {
            let _ = serializer;
            match *self.0 {}
        }
    }
}

/// Return whether metadata contains the exact `key`/`value` pair.
pub fn meta_contains(meta: &Meta, key: &str, value: &str) -> bool {
    #[cfg(feature = "meta-str")]
    {
        meta.iter()
            .any(|(have_key, have_value)| *have_key == key && *have_value == value)
    }
    #[cfg(not(feature = "meta-str"))]
    {
        let _ = (key, value);
        match *meta {}
    }
}

/// Serializable view of one named child.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct NamedView<'a> {
    /// Child name.
    pub name: &'a str,
    /// Child metadata when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<MetaView<'a>>,
}

/// Serializable view of named children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NamedViews<'a>(pub &'a [Named]);

impl Serialize for NamedViews<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for child in self.0 {
            seq.serialize_element(&NamedView {
                name: child.name,
                meta: MetaView::new(child.meta.as_ref()),
            })?;
        }
        seq.end()
    }
}

/// Serializable view of one numbered child.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct NumberedView<'a> {
    /// Child metadata when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<MetaView<'a>>,
}

/// Serializable view of numbered children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NumberedViews<'a>(pub &'a [Numbered]);

impl Serialize for NumberedViews<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for child in self.0 {
            seq.serialize_element(&NumberedView {
                meta: MetaView::new(child.meta.as_ref()),
            })?;
        }
        seq.end()
    }
}

/// Serializable view of a homogeneous child.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct HomogeneousView<'a> {
    /// Child metadata when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<MetaView<'a>>,
}

/// Serializable view of an internal node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum InternalView<'a> {
    /// Named children.
    Named {
        /// Immediate named children.
        children: NamedViews<'a>,
    },
    /// Numbered heterogeneous children.
    Numbered {
        /// Immediate numbered children.
        children: NumberedViews<'a>,
    },
    /// Homogeneous numbered children.
    Homogeneous {
        /// Representative child schema view.
        child: HomogeneousView<'a>,
        /// Number of immediate children.
        len: usize,
    },
}

impl<'a> From<&'a Internal> for InternalView<'a> {
    fn from(internal: &'a Internal) -> Self {
        match internal {
            Internal::Named(children) => Self::Named {
                children: NamedViews(children),
            },
            Internal::Numbered(children) => Self::Numbered {
                children: NumberedViews(children),
            },
            Internal::Homogeneous(child) => Self::Homogeneous {
                child: HomogeneousView {
                    meta: MetaView::new(child.meta.as_ref()),
                },
                len: child.len.get(),
            },
        }
    }
}

/// Serializable view of a schema node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SchemaView<'a> {
    /// Node metadata when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<MetaView<'a>>,
    /// Structured semantics when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sem: Option<&'a Sem>,
    /// Internal node view when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internal: Option<InternalView<'a>>,
}

impl<'a> From<&'a Schema> for SchemaView<'a> {
    fn from(schema: &'a Schema) -> Self {
        Self {
            meta: MetaView::new(schema.meta.as_ref()),
            sem: schema.sem(),
            internal: schema.internal.as_ref().map(InternalView::from),
        }
    }
}

/// A numbered schema item
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize)]
pub struct Numbered {
    /// The child schema
    pub schema: &'static Schema,
    /// The outer metadata
    pub meta: Option<Meta>,
}

impl Numbered {
    /// Create a new Numbered schema item with no outer metadata.
    pub const fn new(schema: &'static Schema) -> Self {
        Self { meta: None, schema }
    }
}

/// A named schema item
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize)]
pub struct Named {
    /// The name of the item
    pub name: &'static str,
    /// The child schema
    pub schema: &'static Schema,
    /// The outer metadata
    pub meta: Option<Meta>,
}

impl Named {
    /// Create a new Named schema item with no outer metadata.
    pub const fn new(name: &'static str, schema: &'static Schema) -> Self {
        Self {
            meta: None,
            name,
            schema,
        }
    }
}

/// A representative schema item for a homogeneous array
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize)]
pub struct Homogeneous {
    /// The number of items
    pub len: NonZero<usize>,
    /// The schema of the child nodes
    pub schema: &'static Schema,
    /// The outer metadata
    pub meta: Option<Meta>,
}

impl Homogeneous {
    /// Create a new Homogeneous schema item with no outer metadata.
    pub const fn new(len: usize, schema: &'static Schema) -> Self {
        Self {
            meta: None,
            len: NonZero::new(len).expect("Must have at least one child"),
            schema,
        }
    }
}

/// An internal node with children
///
/// Always non-empty
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize)]
pub enum Internal {
    /// Named children
    Named(&'static [Named]),
    /// Numbered heterogeneous children
    Numbered(&'static [Numbered]),
    /// Homogeneous numbered children
    Homogeneous(Homogeneous),
}

impl Internal {
    /// Return the number of direct child nodes
    pub const fn len(&self) -> NonZero<usize> {
        match self {
            Self::Named(n) => NonZero::new(n.len()).expect("Must have at least one child"),
            Self::Numbered(n) => NonZero::new(n.len()).expect("Must have at least one child"),
            Self::Homogeneous(h) => h.len,
        }
    }

    /// Return the child schema at the given index
    ///
    /// # Panics
    /// If the index is out of bounds
    pub const fn get_schema(&self, idx: usize) -> &Schema {
        match self {
            Self::Named(nameds) => nameds[idx].schema,
            Self::Numbered(numbereds) => numbereds[idx].schema,
            Self::Homogeneous(homogeneous) => homogeneous.schema,
        }
    }

    /// Return the outer metadata for the given child
    ///
    /// # Panics
    /// If the index is out of bounds
    pub const fn get_meta(&self, idx: usize) -> &Option<Meta> {
        match self {
            Internal::Named(nameds) => &nameds[idx].meta,
            Internal::Numbered(numbereds) => &numbereds[idx].meta,
            Internal::Homogeneous(homogeneous) => &homogeneous.meta,
        }
    }

    /// Perform a index-to-name lookup
    ///
    /// If this succeeds with None, it's a numbered or homogeneous internal node and the
    /// name is the formatted index.
    ///
    /// # Panics
    /// If the index is out of bounds
    pub const fn get_name(&self, idx: usize) -> Option<&str> {
        if let Self::Named(n) = self {
            Some(n[idx].name)
        } else {
            None
        }
    }

    /// Perform a name-to-index lookup
    pub fn get_index(&self, name: &str) -> Option<usize> {
        match self {
            Internal::Named(n) => n.iter().position(|n| n.name == name),
            Internal::Numbered(n) => name.parse().ok().filter(|i| *i < n.len()),
            Internal::Homogeneous(h, ..) => name.parse().ok().filter(|i| *i < h.len.get()),
        }
    }
}

/// The metadata type
///
/// A slice of key-value pairs
#[cfg(feature = "meta-str")]
pub type Meta = &'static [(&'static str, &'static str)];
#[cfg(not(feature = "meta-str"))]
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Ord, Eq, Hash, Serialize)]
/// The metadata type
///
/// Uninhabited
pub enum Meta {}

/// Type of a node: leaf or internal
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize, Default)]
pub struct Schema {
    /// Inner metadata
    pub meta: Option<Meta>,

    /// Structured semantics
    pub sem: MaybeSem,

    /// Internal schemata
    pub internal: Option<Internal>,
}

impl Schema {
    /// A leaf without metadata or sem.
    pub const LEAF: Self = Self {
        meta: None,
        sem: NO_SEM,
        internal: None,
    };

    /// A leaf with a known semantic type.
    pub(crate) const fn leaf_ty(ty: Ty) -> Self {
        #[cfg(feature = "sem")]
        {
            Self {
                meta: None,
                sem: Some(Sem {
                    ty: Some(ty),
                    oneof: false,
                    maybe_absent: false,
                }),
                internal: None,
            }
        }
        #[cfg(not(feature = "sem"))]
        {
            let _ = ty;
            Self::LEAF
        }
    }

    /// Create a new internal node schema with numbered children and without inner metadata
    pub const fn numbered(numbered: &'static [Numbered]) -> Self {
        Self {
            meta: None,
            sem: NO_SEM,
            internal: Some(Internal::Numbered(numbered)),
        }
    }

    /// Create a new internal node schema with named children and without inner metadata
    pub const fn named(named: &'static [Named]) -> Self {
        Self {
            meta: None,
            sem: NO_SEM,
            internal: Some(Internal::Named(named)),
        }
    }

    /// Create a new internal node schema with homogenous children and without innner metadata
    pub const fn homogeneous(homogeneous: Homogeneous) -> Self {
        Self {
            meta: None,
            sem: NO_SEM,
            internal: Some(Internal::Homogeneous(homogeneous)),
        }
    }

    /// Whether this node is a leaf
    pub const fn is_leaf(&self) -> bool {
        self.internal.is_none()
    }

    /// Number of child nodes
    pub const fn len(&self) -> usize {
        match &self.internal {
            None => 0,
            Some(i) => i.len().get(),
        }
    }

    /// See [`Self::is_leaf()`]
    pub const fn is_empty(&self) -> bool {
        self.is_leaf()
    }

    /// Structured semantics when present.
    pub const fn sem(&self) -> Option<&Sem> {
        #[cfg(feature = "sem")]
        {
            self.sem.as_ref()
        }
        #[cfg(not(feature = "sem"))]
        {
            None
        }
    }

    /// Serializable view of this node.
    pub fn view(&'static self) -> SchemaView<'static> {
        self.into()
    }

    /// Serializable view of the node identified by `keys`.
    pub fn get_view(&'static self, keys: impl IntoKeys) -> Result<SchemaView<'static>, KeyError> {
        let mut schema = self;
        let mut keys = keys.into_keys();
        while let Some(internal) = schema.internal.as_ref() {
            match keys.next(internal) {
                Ok(index) => schema = internal.get_schema(index),
                Err(KeyError::TooShort) => break,
                Err(err) => return Err(err),
            }
        }
        keys.finalize()?;
        Ok(schema.into())
    }

    /// Look up the next item from keys and return a child index.
    pub fn next(&self, mut keys: impl Keys) -> Result<usize, KeyError> {
        keys.next(self.internal.as_ref().ok_or(KeyError::TooLong)?)
    }

    /// Traverse from the root to a leaf and call a function for each node.
    ///
    /// If a leaf is found early (`keys` being longer than required)
    /// `Err(KeyError::TooLong)` is returned.
    /// If `keys` is exhausted before reaching a leaf node,
    /// `Err(KeyError::TooShort)` is returned.
    ///
    /// ```
    /// # #[cfg(feature = "derive")] {
    /// # use core::convert::Infallible;
    /// use miniconf::{IntoKeys, TreeSchema};
    /// #[derive(TreeSchema)]
    /// struct S {
    ///     foo: u32,
    ///     bar: [u16; 2],
    /// };
    /// let mut ret = [
    ///     (S::SCHEMA, Some(1usize)),
    ///     (<[u16; 2]>::SCHEMA, Some(0)),
    ///     (u16::SCHEMA, None),
    /// ].into_iter();
    /// let func = |schema, idx_internal: Option<_>| {
    ///     assert_eq!(ret.next().unwrap(), (schema, idx_internal.map(|(idx, _)| idx)));
    ///     Ok::<_, Infallible>(())
    /// };
    /// assert_eq!(S::SCHEMA.descend(["bar", "0"].into_keys(), func), Ok(()));
    /// # }
    /// ```
    ///
    /// # Args
    /// * `keys`: A `Key`s identifying the node.
    /// * `func`: A `FnMut` to be called for each (internal and leaf) node on the path.
    ///   Its arguments are outer schema and optionally the inner index and internal schema.
    ///   Returning `Err(E)` aborts the traversal.
    ///   Returning `Ok(T)` continues the downward traversal.
    ///
    /// # Returns
    /// The leaf `func` call return value.
    pub fn descend<'a, T, E>(
        &'a self,
        mut keys: impl Keys,
        mut func: impl FnMut(&'a Self, Option<(usize, &'a Internal)>) -> Result<T, E>,
    ) -> Result<T, DescendError<E>> {
        let mut schema = self;
        while let Some(internal) = schema.internal.as_ref() {
            let idx = keys.next(internal)?;
            func(schema, Some((idx, internal))).map_err(DescendError::Inner)?;
            schema = internal.get_schema(idx);
        }
        keys.finalize()?;
        func(schema, None).map_err(DescendError::Inner)
    }

    /// Look up outer and inner metadata given keys.
    pub fn get_meta(
        &self,
        keys: impl IntoKeys,
    ) -> Result<(Option<&Option<Meta>>, &Option<Meta>), KeyError> {
        let mut outer = None;
        let mut inner = &self.meta;
        self.descend(keys.into_keys(), |schema, idx_internal| {
            if let Some((idx, internal)) = idx_internal {
                outer = Some(internal.get_meta(idx));
            }
            inner = &schema.meta;
            Ok::<_, Infallible>(())
        })
        .map_err(|e| e.try_into().unwrap())?;
        Ok((outer, inner))
    }

    pub(crate) fn get_indexed(&self, indices: &[usize]) -> &Self {
        let mut schema = self;
        let mut i = 0;
        while i < indices.len() {
            let internal = schema.internal.as_ref().unwrap();
            schema = internal.get_schema(indices[i]);
            i += 1;
        }
        schema
    }

    fn walk(
        &'static self,
        keys: impl IntoKeys,
        mut on_index: impl FnMut(usize, usize) -> bool,
    ) -> Result<Lookup, ResolveError> {
        let mut schema = self;
        let mut keys = keys.into_keys();
        let mut depth = 0;

        while let Some(internal) = schema.internal.as_ref() {
            let idx = match keys.next(internal) {
                Ok(idx) => idx,
                Err(KeyError::TooShort) => {
                    debug_assert!(!schema.is_leaf());
                    return Ok(Lookup { depth, schema });
                }
                Err(err) => {
                    return Err(ResolveError {
                        error: err.into(),
                        depth,
                        leaf: schema.is_leaf().then_some(true),
                    });
                }
            };
            if !on_index(depth, idx) {
                return Err(ResolveError {
                    error: DescendError::Inner(()),
                    depth,
                    leaf: None,
                });
            }
            depth += 1;
            schema = internal.get_schema(idx);
        }

        match keys.finalize() {
            Ok(()) => Ok(Lookup { depth, schema }),
            Err(KeyError::TooLong) => Err(ResolveError {
                error: KeyError::TooLong.into(),
                depth,
                leaf: Some(true),
            }),
            Err(err) => unreachable!("unexpected finalize error: {err:?}"),
        }
    }

    /// Resolve a key traversal while recording the consumed index prefix into `state`.
    ///
    /// On both success and failure, `state[..depth]` contains the longest valid consumed prefix.
    pub fn resolve_into(
        &'static self,
        keys: impl IntoKeys,
        state: &mut [usize],
    ) -> Result<Lookup, ResolveError> {
        self.walk(keys, |depth, idx| {
            let Some(slot) = state.get_mut(depth) else {
                return false;
            };
            *slot = idx;
            true
        })
    }

    /// Get the schema node identified exactly by `keys`.
    pub fn get(&'static self, keys: impl IntoKeys) -> Result<Lookup, KeyError> {
        self.walk(keys, |_, _| true).map_err(|err| match err.error {
            DescendError::Key(err) => err,
            DescendError::Inner(()) => unreachable!("infallible exact lookup"),
        })
    }

    /// Transcode keys to a new keys type representation using its default configuration.
    ///
    /// This is a convenience wrapper around [`FromConfig::transcode()`].
    ///
    /// In order to not require the default configuration, use [`FromConfig::transcode_with`] or
    /// [`Transcode::transcode_from`] on an existing `&mut N`.
    ///
    /// ```
    /// # #[cfg(feature = "derive")] {
    /// use miniconf::{Indices, JsonPath, Lookup, Packed, Path, TreeSchema};
    /// #[derive(TreeSchema)]
    /// struct S {
    ///     foo: u32,
    ///     bar: [u16; 5],
    /// };
    ///
    /// let idx = [1, 1];
    /// let sch = S::SCHEMA;
    ///
    /// let path = sch.transcode::<Path<String>>(idx).unwrap();
    /// assert_eq!(path.path.as_str(), "/bar/1");
    /// let path = sch.transcode::<JsonPath<String>>(idx).unwrap();
    /// assert_eq!(path.0.as_str(), ".bar[1]");
    /// let indices = sch.transcode::<Indices<[usize; 2]>>(&path).unwrap();
    /// assert_eq!(indices.as_ref(), idx);
    /// let indices = sch.transcode::<Indices<[usize; 2]>>(["bar", "1"]).unwrap();
    /// assert_eq!(indices.as_ref(), [1, 1]);
    /// let packed = sch.transcode::<Packed>(["bar", "4"]).unwrap();
    /// assert_eq!(packed.into_lsb().get(), 0b1_1_100);
    /// let path = sch.transcode::<Path<String>>(packed).unwrap();
    /// assert_eq!(path.path.as_str(), "/bar/4");
    /// let lookup = sch.get(&path).unwrap();
    /// assert_eq!((lookup.depth, lookup.schema.is_leaf()), (2, true));
    /// # }
    /// ```
    ///
    /// # Args
    /// * `keys`: `IntoKeys` to identify the node.
    ///
    /// # Returns
    /// The transcoded target on success.
    pub fn transcode<N: Transcode + FromConfig>(
        &self,
        keys: impl IntoKeys,
    ) -> Result<N, DescendError<N::Error>> {
        N::transcode(self, keys)
    }

    /// Transcode keys to a fresh representation using the provided configuration.
    ///
    /// This is a convenience wrapper around [`FromConfig::transcode_with()`].
    pub fn transcode_with<N: Transcode + FromConfig>(
        &self,
        keys: impl IntoKeys,
        config: N::Config,
    ) -> Result<N, DescendError<N::Error>> {
        N::transcode_with(self, keys, config)
    }

    /// The Shape of the schema
    pub const fn shape(&self) -> Shape {
        Shape::new(self)
    }

    /// Return an iterator over nodes of a given type
    ///
    /// This is a walk of all leaf nodes.
    /// The iterator will walk all paths, including those that may be absent at
    /// runtime (see [`TreeSchema#option`]).
    /// The iterator has an exact and trusted `size_hint()`.
    /// The `D` const generic of [`NodeIter`] is the maximum key depth.
    ///
    /// ```
    /// # #[cfg(feature = "derive")] {
    /// use miniconf::{Indices, JsonPath, Lookup, Packed, Path, TreeSchema};
    /// #[derive(TreeSchema)]
    /// struct S {
    ///     foo: u32,
    ///     bar: [u16; 2],
    /// };
    /// const MAX_DEPTH: usize = S::SCHEMA.shape().max_depth;
    /// assert_eq!(MAX_DEPTH, 2);
    ///
    /// let paths: Vec<_> = S::SCHEMA
    ///     .nodes_with::<Path<String>, MAX_DEPTH>('/')
    ///     .map(|p| p.unwrap().into_inner())
    ///     .collect();
    /// assert_eq!(paths, ["/foo", "/bar/0", "/bar/1"]);
    ///
    /// let paths: Vec<_> = S::SCHEMA.nodes::<JsonPath<String>, MAX_DEPTH>()
    ///     .map(|p| p.unwrap().into_inner())
    ///     .collect();
    /// assert_eq!(paths, [".foo", ".bar[0]", ".bar[1]"]);
    ///
    /// let indices: Vec<_> = S::SCHEMA.nodes::<Indices<[_; 2]>, MAX_DEPTH>()
    ///     .map(|p| p.unwrap().into_inner())
    ///     .collect();
    /// assert_eq!(indices, [([0, 0], 1), ([1, 0], 2), ([1, 1], 2)]);
    ///
    /// let packed: Vec<_> = S::SCHEMA.nodes::<Packed, MAX_DEPTH>()
    ///     .map(|p| p.unwrap().into_lsb().get())
    ///     .collect();
    /// assert_eq!(packed, [0b1_0, 0b1_1_0, 0b1_1_1]);
    ///
    /// let nodes: Vec<_> = S::SCHEMA.nodes_with::<Path<String>, MAX_DEPTH>('/')
    ///     .map(|p| {
    ///         let p = p.unwrap();
    ///         let lookup = S::SCHEMA.get(&p).unwrap();
    ///         ((lookup.depth, lookup.schema.is_leaf()), p.into_inner())
    ///     })
    ///     .collect();
    /// assert_eq!(
    ///     nodes,
    ///     [
    ///         ((1, true), "/foo".into()),
    ///         ((2, true), "/bar/0".into()),
    ///         ((2, true), "/bar/1".into()),
    ///     ]
    /// );
    /// # }
    /// ```
    pub const fn nodes<N: FromConfig, const D: usize>(&'static self) -> ExactSize<NodeIter<N, D>> {
        NodeIter::new(self, [0; D], 0, N::DEFAULT_CONFIG).exact_size()
    }

    /// Return an iterator over nodes using a preconfigured output seed.
    ///
    /// This is useful for runtime-configured path encodings such as [`Path`],
    /// where the emitted separator is stored in the target value rather than in a const generic.
    pub fn nodes_with<N: FromConfig, const D: usize>(
        &'static self,
        config: N::Config,
    ) -> ExactSize<NodeIter<N, D>> {
        NodeIter::new(self, [0; D], 0, config).exact_size()
    }
}
