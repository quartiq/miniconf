use core::marker::PhantomData;

use crosstrait::{entry, Registry};
use postcard_schema::schema::NamedType;
use serde::Serialize;

use miniconf::{IntoKeys, Traversal, TreeAny, TreeKey};

mod node;
use node::Node;
mod common;

/// dyn compatible postcard schema trait
trait Schema {
    fn schema(&self) -> &'static NamedType;
}

impl<T: postcard_schema::Schema> Schema for T {
    fn schema(&self) -> &'static NamedType {
        Self::SCHEMA
    }
}

/// Graph of `Node` for a Tree type
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Graph<T> {
    root: Node<&'static NamedType>,
    _t: PhantomData<T>,
}

impl<T: TreeKey> Default for Graph<T> {
    fn default() -> Self {
        Self {
            root: T::traverse_all(),
            _t: PhantomData,
        }
    }
}

impl<T> Graph<T> {
    /// Return a reference to the root node
    pub fn root(&self) -> &Node<&'static NamedType> {
        &self.root
    }

    /// Trace all leaf values
    pub fn trace(&mut self, registry: &Registry, value: &T)
    where
        T: TreeAny,
    {
        self.root
            .visit_mut(&mut vec![], &mut |idx, node| {
                if let Node::Leaf(schema) = node {
                    match value.ref_any_by_key(idx.into_keys()) {
                        Ok(any) => {
                            if let Some(n) = registry.cast_ref::<dyn Schema>(any) {
                                schema.replace(n.schema());
                            }
                        }
                        Err(Traversal::Absent(_depth) | Traversal::Access(_depth, _)) => {}
                        _ => unreachable!(),
                    }
                }
                Ok::<_, ()>(())
            })
            .unwrap();
    }
}

fn main() -> anyhow::Result<()> {
    let registry = Registry::new(&[
        entry!(bool => dyn Schema),
        entry!(i32 => dyn Schema),
        entry!(Option<i32> => dyn Schema),
        entry!([i32; 2] => dyn Schema),
        // entry!(common::Inner => dyn Schema),
        // entry!(common::Either => dyn Schema),
    ]);

    let mut settings = common::Settings::new();
    settings.enable();

    let mut graph = Graph::default();

    graph.trace(&registry, &settings);
    println!("{}", serde_json::to_string_pretty(graph.root())?);

    Ok(())
}
