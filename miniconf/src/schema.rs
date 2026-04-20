use core::{convert::Infallible, num::NonZero};
use serde::{
    Serialize, Serializer,
    ser::{SerializeMap as _, SerializeStruct as _},
};

use crate::{DescendError, ExactSize, IntoKeys, KeyError, Keys, NodeIter, Shape, Transcode};

#[cfg(feature = "sem")]
type StoredSem = Sem;
#[cfg(not(feature = "sem"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct StoredSem;

#[cfg(feature = "meta-node")]
type StoredNodeMeta = Meta;
#[cfg(not(feature = "meta-node"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct StoredNodeMeta;

#[cfg(feature = "meta-edge")]
type StoredEdgeMeta = Meta;
#[cfg(not(feature = "meta-edge"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct StoredEdgeMeta;

/// Structured semantics for a mutually exclusive named node.
pub const ONEOF_SEM: Sem = Sem::new(None, true, false);
/// Structured semantics for a node that may be absent at runtime.
pub const MAYBE_ABSENT_SEM: Sem = Sem::new(None, false, true);
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
    /// The longest valid consumed prefix.
    pub lookup: Lookup,
}

/// Structured machine-readable schema semantics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[non_exhaustive]
pub struct Sem {
    /// Semantic leaf type when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    ty: Option<Ty>,
    /// This named internal node has mutually exclusive children.
    #[serde(skip_serializing_if = "core::ops::Not::not")]
    oneof: bool,
    /// This node may be absent at runtime.
    #[serde(skip_serializing_if = "core::ops::Not::not")]
    maybe_absent: bool,
}

impl Sem {
    /// Empty structured semantics.
    pub const EMPTY: Self = Self::new(None, false, false);

    /// Construct structured schema semantics.
    pub const fn new(ty: Option<Ty>, oneof: bool, maybe_absent: bool) -> Self {
        Self {
            ty,
            oneof,
            maybe_absent,
        }
    }

    /// Semantic leaf type when known.
    pub const fn ty(&self) -> Option<Ty> {
        self.ty
    }

    /// Whether the node has mutually exclusive children.
    pub const fn oneof(&self) -> bool {
        self.oneof
    }

    /// Whether the node may be absent at runtime.
    pub const fn maybe_absent(&self) -> bool {
        self.maybe_absent
    }

    /// Whether there is no semantic payload.
    pub const fn is_empty(&self) -> bool {
        self.ty.is_none() && !self.oneof && !self.maybe_absent
    }
}

#[cfg(feature = "sem")]
const fn store_sem(sem: Sem) -> StoredSem {
    sem
}

#[cfg(not(feature = "sem"))]
const fn store_sem(_sem: Sem) -> StoredSem {
    StoredSem
}

#[cfg(feature = "sem")]
const fn sem_ref(sem: &StoredSem) -> Option<&Sem> {
    Some(sem)
}

#[cfg(not(feature = "sem"))]
const fn sem_ref(_sem: &StoredSem) -> Option<&Sem> {
    None
}

#[cfg(feature = "meta-node")]
const fn store_node_meta(meta: Meta) -> StoredNodeMeta {
    meta
}

#[cfg(not(feature = "meta-node"))]
const fn store_node_meta(_meta: Meta) -> StoredNodeMeta {
    StoredNodeMeta
}

#[cfg(feature = "meta-node")]
const fn node_meta_ref(meta: &StoredNodeMeta) -> &Meta {
    meta
}

#[cfg(not(feature = "meta-node"))]
const fn node_meta_ref(_meta: &StoredNodeMeta) -> &Meta {
    &Meta::EMPTY
}

#[cfg(feature = "meta-edge")]
const fn store_edge_meta(meta: Meta) -> StoredEdgeMeta {
    meta
}

#[cfg(not(feature = "meta-edge"))]
const fn store_edge_meta(_meta: Meta) -> StoredEdgeMeta {
    StoredEdgeMeta
}

#[cfg(feature = "meta-edge")]
const fn edge_meta_ref(meta: &StoredEdgeMeta) -> &Meta {
    meta
}

#[cfg(not(feature = "meta-edge"))]
const fn edge_meta_ref(_meta: &StoredEdgeMeta) -> &Meta {
    &Meta::EMPTY
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

/// A numbered schema item
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct Numbered {
    /// The child schema
    pub(crate) schema: &'static Schema,
    /// The edge metadata
    pub(crate) meta: StoredEdgeMeta,
}

impl Numbered {
    /// Create a new Numbered schema item with no edge metadata.
    pub const fn new(schema: &'static Schema, meta: Meta) -> Self {
        Self {
            schema,
            meta: store_edge_meta(meta),
        }
    }

    /// The child schema.
    pub const fn schema(&self) -> &'static Schema {
        self.schema
    }

    /// Edge metadata when present.
    pub const fn edge_meta(&self) -> &Meta {
        edge_meta_ref(&self.meta)
    }
}

impl Serialize for Numbered {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer
            .serialize_struct("Numbered", 1 + usize::from(!self.edge_meta().is_empty()))?;
        state.serialize_field("schema", self.schema())?;
        if !self.edge_meta().is_empty() {
            state.serialize_field("meta", self.edge_meta())?;
        }
        state.end()
    }
}

/// A named schema item
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct Named {
    /// The name of the item
    pub(crate) name: &'static str,
    /// The child schema
    pub(crate) schema: &'static Schema,
    /// The edge metadata
    pub(crate) meta: StoredEdgeMeta,
}

impl Named {
    /// Create a new Named schema item with no edge metadata.
    pub const fn new(name: &'static str, schema: &'static Schema, meta: Meta) -> Self {
        Self {
            name,
            schema,
            meta: store_edge_meta(meta),
        }
    }

    /// The child name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The child schema.
    pub const fn schema(&self) -> &'static Schema {
        self.schema
    }

    /// Edge metadata when present.
    pub const fn edge_meta(&self) -> &Meta {
        edge_meta_ref(&self.meta)
    }
}

impl Serialize for Named {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state =
            serializer.serialize_struct("Named", 2 + usize::from(!self.edge_meta().is_empty()))?;
        state.serialize_field("name", self.name())?;
        state.serialize_field("schema", self.schema())?;
        if !self.edge_meta().is_empty() {
            state.serialize_field("meta", self.edge_meta())?;
        }
        state.end()
    }
}

/// A representative schema item for a homogeneous array
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct Homogeneous {
    /// The number of items
    pub(crate) len: NonZero<usize>,
    /// The schema of the child nodes
    pub(crate) schema: &'static Schema,
    /// The edge metadata
    pub(crate) meta: StoredEdgeMeta,
}

impl Homogeneous {
    /// Create a new Homogeneous schema item with no edge metadata.
    pub const fn new(len: usize, schema: &'static Schema, meta: Meta) -> Self {
        Self {
            len: NonZero::new(len).expect("Must have at least one child"),
            schema,
            meta: store_edge_meta(meta),
        }
    }

    /// The number of items.
    pub const fn len(&self) -> NonZero<usize> {
        self.len
    }

    /// The child schema.
    pub const fn schema(&self) -> &'static Schema {
        self.schema
    }

    /// Edge metadata when present.
    pub const fn edge_meta(&self) -> &Meta {
        edge_meta_ref(&self.meta)
    }
}

impl Serialize for Homogeneous {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer
            .serialize_struct("Homogeneous", 2 + usize::from(!self.edge_meta().is_empty()))?;
        state.serialize_field("len", &self.len())?;
        state.serialize_field("schema", self.schema())?;
        if !self.edge_meta().is_empty() {
            state.serialize_field("meta", self.edge_meta())?;
        }
        state.end()
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

    /// Return the edge metadata for the given child
    ///
    /// # Panics
    /// If the index is out of bounds
    pub const fn get_edge_meta(&self, idx: usize) -> &Meta {
        match self {
            Internal::Named(nameds) => nameds[idx].edge_meta(),
            Internal::Numbered(numbereds) => numbereds[idx].edge_meta(),
            Internal::Homogeneous(homogeneous) => homogeneous.edge_meta(),
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

/// Immutable schema metadata.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd, Ord, Eq, Hash)]
pub struct Meta {
    /// Backing storage for metadata items.
    pub items: &'static [(&'static str, &'static str)],
}

impl Meta {
    /// Empty metadata.
    pub const EMPTY: Self = Self { items: &[] };

    /// Construct metadata from a static list of key/value pairs.
    pub const fn new(items: &'static [(&'static str, &'static str)]) -> Self {
        Self { items }
    }

    /// Whether the metadata bag is empty.
    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Return the first metadata value for `key`.
    pub fn get(&self, key: &str) -> Option<&'static str> {
        self.items
            .iter()
            .find_map(|(have_key, have_value)| (*have_key == key).then_some(*have_value))
    }
}

impl Serialize for Meta {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.items.len()))?;
        for (key, value) in self.items {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

/// Shared static schema payload for one tree node.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct NodeSchema {
    meta: StoredNodeMeta,
    sem: StoredSem,
}

impl NodeSchema {
    /// Empty node metadata and semantics.
    pub const EMPTY: Self = Self {
        meta: store_node_meta(Meta::EMPTY),
        sem: store_sem(Sem::EMPTY),
    };

    /// Construct a node schema from node metadata and semantics.
    pub const fn new(meta: Meta, sem: Sem) -> Self {
        Self {
            meta: store_node_meta(meta),
            sem: store_sem(sem),
        }
    }

    const fn sem(&self) -> Option<&Sem> {
        sem_ref(&self.sem)
    }

    const fn node_meta(&self) -> &Meta {
        node_meta_ref(&self.meta)
    }
}

/// Static schema payload for an internal node.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct InternalSchema {
    node: NodeSchema,
    internal: Internal,
}

impl InternalSchema {
    /// Construct an internal schema from node payload and child layout.
    pub const fn new(node: NodeSchema, internal: Internal) -> Self {
        Self { node, internal }
    }

    const fn sem(&self) -> Option<&Sem> {
        self.node.sem()
    }

    const fn node_meta(&self) -> &Meta {
        self.node.node_meta()
    }

    /// Child layout strategy.
    pub const fn internal(&self) -> &Internal {
        &self.internal
    }
}

/// Static schema for one tree node.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum Schema {
    /// Leaf node without children.
    Leaf(NodeSchema),
    /// Internal node with one child layout strategy.
    Internal(InternalSchema),
}

impl Default for Schema {
    fn default() -> Self {
        Self::LEAF
    }
}

impl Serialize for Schema {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut len =
            usize::from(!self.node_meta().is_empty()) + usize::from(self.internal().is_some());
        if let Some(sem) = self.sem()
            && !sem.is_empty()
        {
            len += 1;
        }
        let mut state = serializer.serialize_struct("Schema", len)?;
        self.serialize_fields(&mut state)?;
        state.end()
    }
}

impl Schema {
    /// A leaf without metadata or sem.
    pub const LEAF: Self = Self::Leaf(NodeSchema::EMPTY);

    /// Construct a leaf schema from node metadata and semantics.
    pub const fn leaf(meta: Meta, sem: Sem) -> Self {
        Self::Leaf(NodeSchema::new(meta, sem))
    }

    /// Construct a schema with the same shape and new node metadata and semantics.
    pub const fn rebuild(&self, meta: Meta, sem: Sem) -> Self {
        let node = NodeSchema::new(meta, sem);
        match self {
            Self::Leaf(_) => Self::Leaf(node),
            Self::Internal(schema) => Self::Internal(InternalSchema::new(node, schema.internal)),
        }
    }

    /// A leaf with a known semantic type.
    pub(crate) const fn leaf_ty(ty: Ty) -> Self {
        Self::leaf(Meta::EMPTY, Sem::new(Some(ty), false, false))
    }

    /// Create a new internal node schema with numbered children and without node metadata
    pub const fn numbered(numbered: &'static [Numbered]) -> Self {
        Self::Internal(InternalSchema::new(
            NodeSchema::EMPTY,
            Internal::Numbered(numbered),
        ))
    }

    /// Create a new internal node schema with named children and without node metadata
    pub const fn named(named: &'static [Named]) -> Self {
        Self::Internal(InternalSchema::new(
            NodeSchema::EMPTY,
            Internal::Named(named),
        ))
    }

    /// Create a new internal node schema with homogenous children and without node metadata
    pub const fn homogeneous(homogeneous: Homogeneous) -> Self {
        Self::Internal(InternalSchema::new(
            NodeSchema::EMPTY,
            Internal::Homogeneous(homogeneous),
        ))
    }

    fn serialize_fields<S>(&self, state: &mut S) -> Result<(), S::Error>
    where
        S: serde::ser::SerializeStruct,
    {
        let node_meta = self.node_meta();
        let sem = self.sem();
        if !self.node_meta().is_empty() {
            state.serialize_field("meta", node_meta)?;
        }
        if let Some(sem) = sem
            && !sem.is_empty()
        {
            state.serialize_field("sem", sem)?;
        }
        if let Some(internal) = self.internal() {
            state.serialize_field("internal", internal)?;
        }
        Ok(())
    }

    /// Whether this node is a leaf
    pub const fn is_leaf(&self) -> bool {
        matches!(self, Self::Leaf(_))
    }

    /// Number of child nodes
    pub const fn len(&self) -> usize {
        match self {
            Self::Leaf(_) => 0,
            Self::Internal(schema) => schema.internal().len().get(),
        }
    }

    /// See [`Self::is_leaf()`]
    pub const fn is_empty(&self) -> bool {
        self.is_leaf()
    }

    /// Structured semantics when present.
    pub const fn sem(&self) -> Option<&Sem> {
        match self {
            Self::Leaf(schema) => schema.sem(),
            Self::Internal(schema) => schema.sem(),
        }
    }

    /// Node metadata.
    pub const fn node_meta(&self) -> &Meta {
        match self {
            Self::Leaf(node) => node.node_meta(),
            Self::Internal(schema) => schema.node_meta(),
        }
    }

    /// Node metadata value for `key` when present.
    pub fn node_meta_value(&self, key: &str) -> Option<&'static str> {
        self.node_meta().get(key)
    }

    /// Internal schema when present.
    pub const fn internal(&self) -> Option<&Internal> {
        match self {
            Self::Leaf(_) => None,
            Self::Internal(schema) => Some(schema.internal()),
        }
    }

    /// Resolve the next selector segment from a normalized key cursor.
    pub fn next(&self, mut keys: impl Keys) -> Result<usize, KeyError> {
        keys.next(self.internal().ok_or(KeyError::TooLong)?)
    }

    /// Traverse from the root to a leaf using a normalized key cursor.
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
    /// * `keys`: A normalized [`Keys`] cursor identifying the node.
    /// * `func`: A `FnMut` to be called for each (internal and leaf) node on the path.
    ///   Its arguments are the current node schema and optionally the traversed edge index and
    ///   internal schema.
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
        while let Some(internal) = schema.internal() {
            let idx = keys.next(internal)?;
            func(schema, Some((idx, internal))).map_err(DescendError::Inner)?;
            schema = internal.get_schema(idx);
        }
        keys.finalize()?;
        func(schema, None).map_err(DescendError::Inner)
    }

    /// Look up edge and node metadata given a boundary key input.
    pub fn get_meta(&self, keys: impl IntoKeys) -> Result<(Option<&Meta>, &Meta), KeyError> {
        let mut edge = None;
        let mut node = self.node_meta();
        self.descend(keys.into_keys(), |schema, idx_internal| {
            if let Some((idx, internal)) = idx_internal {
                edge = Some(internal.get_edge_meta(idx));
            }
            node = schema.node_meta();
            Ok::<_, Infallible>(())
        })
        .map_err(|e| e.try_into().unwrap())?;
        Ok((edge, node))
    }

    fn walk(
        &'static self,
        mut keys: impl Keys,
        mut on_index: impl FnMut(usize, usize) -> bool,
    ) -> Result<Lookup, ResolveError> {
        let mut schema = self;
        let mut depth = 0;

        while let Some(internal) = schema.internal() {
            let idx = match keys.next(internal) {
                Ok(idx) => idx,
                Err(KeyError::TooShort) => {
                    debug_assert!(!schema.is_leaf());
                    return Ok(Lookup { depth, schema });
                }
                Err(err) => {
                    return Err(ResolveError {
                        error: err.into(),
                        lookup: Lookup { depth, schema },
                    });
                }
            };
            if !on_index(depth, idx) {
                return Err(ResolveError {
                    error: DescendError::Inner(()),
                    lookup: Lookup { depth, schema },
                });
            }
            depth += 1;
            schema = internal.get_schema(idx);
        }

        match keys.finalize() {
            Ok(()) => Ok(Lookup { depth, schema }),
            Err(KeyError::TooLong) => Err(ResolveError {
                error: KeyError::TooLong.into(),
                lookup: Lookup { depth, schema },
            }),
            Err(err) => unreachable!("unexpected finalize error: {err:?}"),
        }
    }

    /// Resolve a boundary key input while recording the consumed index prefix into `state`.
    ///
    /// On both success and failure, `state[..depth]` contains the longest valid consumed prefix.
    pub fn resolve_into(
        &'static self,
        keys: impl IntoKeys,
        state: &mut [usize],
    ) -> Result<Lookup, ResolveError> {
        self.walk(keys.into_keys(), |depth, idx| {
            let Some(slot) = state.get_mut(depth) else {
                return false;
            };
            *slot = idx;
            true
        })
    }

    /// Get the schema node identified exactly by a boundary key input.
    pub fn get(&'static self, keys: impl IntoKeys) -> Result<Lookup, KeyError> {
        self.walk(keys.into_keys(), |_, _| true)
            .map_err(|err| match err.error {
                DescendError::Key(err) => err,
                DescendError::Inner(()) => unreachable!("infallible exact lookup"),
            })
    }

    /// Transcode a boundary key input to a new key representation using its default configuration.
    ///
    /// This default-constructs the output and then calls [`Transcode::transcode_from()`].
    ///
    /// ```
    /// # #[cfg(feature = "derive")] {
    /// use miniconf::{ConstPath, Indices, JsonPath, JsonPathIter, Lookup, Packed, Path, TreeSchema};
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
    /// let path = sch.transcode::<ConstPath<String, ':'>>(idx).unwrap();
    /// assert_eq!(path.0.as_str(), ":bar:1");
    /// let path = sch.transcode::<JsonPath<String>>(idx).unwrap();
    /// assert_eq!(path.0.as_str(), ".bar[1]");
    /// let indices = sch
    ///     .transcode::<Indices<[usize; 2]>>(JsonPathIter::new(path.0.as_str()))
    ///     .unwrap();
    /// assert_eq!(indices.as_ref(), idx);
    /// let indices = sch.transcode::<Indices<[usize; 2]>>(["bar", "1"]).unwrap();
    /// assert_eq!(indices.as_ref(), [1, 1]);
    /// let packed = sch.transcode::<Packed>(["bar", "4"]).unwrap();
    /// assert_eq!(packed.into_lsb().get(), 0b1_1_100);
    /// let path = sch.transcode::<Path<String>>(packed).unwrap();
    /// assert_eq!(path.path.as_str(), "/bar/4");
    /// let lookup = sch.get(path.as_ref()).unwrap();
    /// assert_eq!((lookup.depth, lookup.schema.is_leaf()), (2, true));
    /// # }
    /// ```
    ///
    /// # Args
    /// * `keys`: A boundary [`IntoKeys`] input identifying the node.
    ///
    /// # Returns
    /// The transcoded target on success.
    pub fn transcode<N: Transcode + Default>(
        &self,
        keys: impl IntoKeys,
    ) -> Result<N, DescendError<N::Error>> {
        N::transcode(self, keys)
    }

    /// Summary bounds and counts for this schema.
    pub const fn shape(&self) -> Shape {
        Shape::new(self)
    }

    /// The maximum key depth.
    pub const fn max_depth(&self) -> usize {
        self.shape().max_depth
    }

    /// The maximum path length in bytes including `separator`.
    pub const fn max_length(&self, separator: &str) -> usize {
        self.shape().max_length(separator)
    }

    /// Return an iterator over nodes of a given type
    ///
    /// This is a walk of all leaf nodes.
    /// The iterator will walk all paths, including those that may be absent at
    /// runtime (see [the `Option` section on `TreeSchema`](crate::TreeSchema#option)).
    /// The iterator has an exact and trusted `size_hint()`.
    /// The `D` const generic of [`NodeIter`] is the maximum key depth.
    ///
    /// ```
    /// # #[cfg(feature = "derive")] {
    /// use miniconf::{ConstPath, Indices, JsonPath, Lookup, Packed, Path, TreeSchema};
    /// #[derive(TreeSchema)]
    /// struct S {
    ///     foo: u32,
    ///     bar: [u16; 2],
    /// };
    /// const MAX_DEPTH: usize = S::SCHEMA.max_depth();
    /// assert_eq!(MAX_DEPTH, 2);
    ///
    /// let paths: Vec<_> = S::SCHEMA
    ///     .nodes::<Path<String>, MAX_DEPTH>()
    ///     .map(|p| p.unwrap().into_inner())
    ///     .collect();
    /// assert_eq!(paths, ["/foo", "/bar/0", "/bar/1"]);
    ///
    /// let paths: Vec<_> = S::SCHEMA.nodes::<ConstPath<String, ':'>, MAX_DEPTH>()
    ///     .map(|p| p.unwrap().into_inner())
    ///     .collect();
    /// assert_eq!(paths, [":foo", ":bar:0", ":bar:1"]);
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
    /// let nodes: Vec<_> = S::SCHEMA.nodes::<Path<String>, MAX_DEPTH>()
    ///     .map(|p| {
    ///         let p = p.unwrap();
    ///         let lookup = S::SCHEMA.get(p.as_ref()).unwrap();
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
    pub const fn nodes<N: Transcode + Default, const D: usize>(
        &'static self,
    ) -> ExactSize<NodeIter<N, D>> {
        NodeIter::<N, D>::new(self, [0; D], 0).exact_size()
    }
}
