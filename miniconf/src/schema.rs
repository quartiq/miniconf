use core::{convert::Infallible, num::NonZero};
use serde::Serialize;

use crate::{
    DescendError, ExactSize, FromConfig, IntoKeys, KeyError, Keys, NodeIter, Shape, Transcode,
};

/// Result of an exact key lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Lookup {
    /// The number of keys consumed.
    pub depth: usize,
    /// Whether the traversal reached a leaf node.
    pub leaf: bool,
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

/// A numbered schema item
#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize)]
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
#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize)]
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
#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize)]
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
#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize)]
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
#[cfg(not(any(feature = "meta-str")))]
#[derive(Debug, Clone, PartialEq, PartialOrd, Ord, Eq, Hash, Serialize)]
/// The metadata type
///
/// Uninhabited
pub enum Meta {}

/// Type of a node: leaf or internal
#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize, Default)]
pub struct Schema {
    /// Inner metadata
    pub meta: Option<Meta>,

    /// Internal schemata
    pub internal: Option<Internal>,
}

impl Schema {
    /// A leaf without metadata
    pub const LEAF: Self = Self {
        meta: None,
        internal: None,
    };

    /// Create a new internal node schema with numbered children and without inner metadata
    pub const fn numbered(numbered: &'static [Numbered]) -> Self {
        Self {
            meta: None,
            internal: Some(Internal::Numbered(numbered)),
        }
    }

    /// Create a new internal node schema with named children and without inner metadata
    pub const fn named(named: &'static [Named]) -> Self {
        Self {
            meta: None,
            internal: Some(Internal::Named(named)),
        }
    }

    /// Create a new internal node schema with homogenous children and without innner metadata
    pub const fn homogeneous(homogeneous: Homogeneous) -> Self {
        Self {
            meta: None,
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

    /// Look up the next item from keys and return a child index
    ///
    /// # Panics
    /// On a leaf Schema.
    pub fn next(&self, mut keys: impl Keys) -> Result<usize, KeyError> {
        keys.next(self.internal.as_ref().unwrap())
    }

    /// Traverse from the root to a leaf and call a function for each node.
    ///
    /// If a leaf is found early (`keys` being longer than required)
    /// `Err(KeyError::TooLong)` is returned.
    /// If `keys` is exhausted before reaching a leaf node,
    /// `Err(KeyError::TooShort)` is returned.
    ///
    /// ```
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
        mut on_index: impl FnMut(usize, usize) -> Result<(), ()>,
    ) -> Result<Lookup, ResolveError> {
        let mut schema = self;
        let mut keys = keys.into_keys();
        let mut depth = 0;

        while let Some(internal) = schema.internal.as_ref() {
            let idx = match keys.next(internal) {
                Ok(idx) => idx,
                Err(KeyError::TooShort) => {
                    let leaf = schema.is_leaf();
                    debug_assert!(!leaf);
                    return Ok(Lookup {
                        depth,
                        leaf,
                        schema,
                    });
                }
                Err(err) => {
                    return Err(ResolveError {
                        error: err.into(),
                        depth,
                        leaf: schema.is_leaf().then_some(true),
                    });
                }
            };
            on_index(depth, idx).map_err(|()| ResolveError {
                error: DescendError::Inner(()),
                depth,
                leaf: None,
            })?;
            depth += 1;
            schema = internal.get_schema(idx);
        }

        match keys.finalize() {
            Ok(()) => Ok(Lookup {
                depth,
                leaf: true,
                schema,
            }),
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
            *state.get_mut(depth).ok_or(())? = idx;
            Ok(())
        })
    }

    /// Get the schema node identified exactly by `keys`.
    pub fn get(&'static self, keys: impl IntoKeys) -> Result<Lookup, ResolveError> {
        self.walk(keys, |_, _| Ok(()))
    }

    /// Transcode keys to a new keys type representation using its default configuration.
    ///
    /// This is a convenience wrapper around [`FromConfig::transcode()`].
    ///
    /// In order to not require the default configuration, use [`FromConfig::transcode_with`] or
    /// [`Transcode::transcode_from`] on an existing `&mut N`.
    ///
    /// ```
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
    /// assert_eq!((lookup.depth, lookup.leaf), (2, true));
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
    /// runtime (see [`crate::TreeSchema#option`]).
    /// The iterator has an exact and trusted `size_hint()`.
    /// The `D` const generic of [`NodeIter`] is the maximum key depth.
    ///
    /// ```
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
    ///         ((lookup.depth, lookup.leaf), p.into_inner())
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
    /// ```
    pub const fn nodes<N: FromConfig, const D: usize>(&'static self) -> ExactSize<NodeIter<N, D>> {
        NodeIter::new(self, [0; D], 0, N::DEFAULT_CONFIG).exact_size()
    }

    /// Return an iterator over nodes using a preconfigured output seed.
    ///
    /// This is useful for runtime-configured path encodings such as [`crate::Path`],
    /// where the emitted separator is stored in the target value rather than in a const generic.
    pub fn nodes_with<N: FromConfig, const D: usize>(
        &'static self,
        config: N::Config,
    ) -> ExactSize<NodeIter<N, D>> {
        NodeIter::new(self, [0; D], 0, config).exact_size()
    }
}
