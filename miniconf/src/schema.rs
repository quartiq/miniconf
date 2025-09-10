use core::{convert::Infallible, num::NonZero};
use serde::Serialize;

use crate::{DescendError, ExactSize, IntoKeys, KeyError, Keys, NodeIter, Shape, Transcode};

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
    #[inline]
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
    #[inline]
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
    #[inline]
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
    #[inline]
    pub const fn get_name(&self, idx: usize) -> Option<&str> {
        if let Self::Named(n) = self {
            Some(n[idx].name)
        } else {
            None
        }
    }

    /// Perform a name-to-index lookup
    #[inline]
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

    /// Create a new internal node schema with named children and without innner metadata
    pub const fn numbered(numbered: &'static [Numbered]) -> Self {
        Self {
            meta: None,
            internal: Some(Internal::Numbered(numbered)),
        }
    }

    /// Create a new internal node schema with numbered children and without innner metadata
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
    #[inline]
    pub const fn is_leaf(&self) -> bool {
        self.internal.is_none()
    }

    /// Number of child nodes
    #[inline]
    pub const fn len(&self) -> usize {
        match &self.internal {
            None => 0,
            Some(i) => i.len().get(),
        }
    }

    /// See [`Self::is_leaf()`]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.is_leaf()
    }

    /// Look up the next item from keys and return a child index
    ///
    /// # Panics
    /// On a leaf Schema.
    #[inline]
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
    #[inline]
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

    /// Get the schema of the node identified by keys.
    pub fn get(&self, keys: impl IntoKeys) -> Result<&Self, KeyError> {
        let mut schema = self;
        self.descend(keys.into_keys(), |s, _idx_internal| {
            schema = s;
            Ok::<_, Infallible>(())
        })
        .map_err(|e| e.try_into().unwrap())?;
        Ok(schema)
    }

    /// Transcode keys to a new keys type representation
    ///
    /// The keys can be
    /// * too short: the internal node is returned
    /// * matched length: the leaf node is returned
    /// * too long: Err(TooLong(depth)) is returned
    ///
    /// In order to not require `N: Default`, use [`Transcode::transcode`] on
    /// an existing `&mut N`.
    ///
    /// ```
    /// use miniconf::{Indices, JsonPath, Packed, Track, Short, Path, TreeSchema};
    /// #[derive(TreeSchema)]
    /// struct S {
    ///     foo: u32,
    ///     bar: [u16; 5],
    /// };
    ///
    /// let idx = [1, 1];
    /// let sch = S::SCHEMA;
    ///
    /// let path = sch.transcode::<Path<String, '/'>>(idx).unwrap();
    /// assert_eq!(path.0.as_str(), "/bar/1");
    /// let path = sch.transcode::<JsonPath<String>>(idx).unwrap();
    /// assert_eq!(path.0.as_str(), ".bar[1]");
    /// let indices = sch.transcode::<Indices<[usize; 2]>>(&path).unwrap();
    /// assert_eq!(indices.as_ref(), idx);
    /// let indices = sch.transcode::<Indices<[usize; 2]>>(["bar", "1"]).unwrap();
    /// assert_eq!(indices.as_ref(), [1, 1]);
    /// let packed = sch.transcode::<Packed>(["bar", "4"]).unwrap();
    /// assert_eq!(packed.into_lsb().get(), 0b1_1_100);
    /// let path = sch.transcode::<Path<String, '/'>>(packed).unwrap();
    /// assert_eq!(path.0.as_str(), "/bar/4");
    /// let node = sch.transcode::<Short<Track<()>>>(&path).unwrap();
    /// assert_eq!((node.leaf(), node.inner().depth()), (true, 2));
    /// ```
    ///
    /// # Args
    /// * `keys`: `IntoKeys` to identify the node.
    ///
    /// # Returns
    /// Transcoded target and node information on success
    #[inline]
    pub fn transcode<N: Transcode + Default>(
        &self,
        keys: impl IntoKeys,
    ) -> Result<N, DescendError<N::Error>> {
        let mut target = N::default();
        target.transcode(self, keys)?;
        Ok(target)
    }

    /// The Shape of the schema
    #[inline]
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
    /// use miniconf::{Indices, JsonPath, Short, Track, Packed, Path, TreeSchema};
    /// #[derive(TreeSchema)]
    /// struct S {
    ///     foo: u32,
    ///     bar: [u16; 2],
    /// };
    /// const MAX_DEPTH: usize = S::SCHEMA.shape().max_depth;
    /// assert_eq!(MAX_DEPTH, 2);
    ///
    /// let paths: Vec<_> = S::SCHEMA.nodes::<Path<String, '/'>, MAX_DEPTH>()
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
    /// let nodes: Vec<_> = S::SCHEMA.nodes::<Short<Track<()>>, MAX_DEPTH>()
    ///     .map(|p| {
    ///         let p = p.unwrap();
    ///         (p.leaf(), p.inner().depth())
    ///     })
    ///     .collect();
    /// assert_eq!(nodes, [(true, 1), (true, 2), (true, 2)]);
    /// ```
    #[inline]
    pub const fn nodes<N: Transcode + Default, const D: usize>(
        &'static self,
    ) -> ExactSize<NodeIter<N, D>> {
        NodeIter::exact_size(self)
    }
}
