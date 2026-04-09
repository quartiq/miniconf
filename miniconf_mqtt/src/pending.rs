use heapless::String;
use miniconf::{DescendError, NodeIter, Path, Schema};

use crate::{MAX_TOPIC_LENGTH, SEPARATOR, protocol::ReplyTarget};

type PathIter<const Y: usize> = NodeIter<Path<String<MAX_TOPIC_LENGTH>>, Y>;

pub(crate) enum Pending<const Y: usize> {
    Idle,
    List {
        iter: PathIter<Y>,
        reply: ReplyTarget,
    },
    Dump {
        iter: PathIter<Y>,
    },
}

impl<const Y: usize> Pending<Y> {
    pub(crate) const fn new() -> Self {
        Self::Idle
    }

    pub(crate) const fn is_active(&self) -> bool {
        !matches!(self, Self::Idle)
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::Idle;
    }

    pub(crate) fn dump(
        schema: &'static Schema,
        path: Option<&str>,
    ) -> Result<Self, DescendError<()>> {
        let iter = match path {
            Some(path) => NodeIter::with_root(schema, Path::new(path, SEPARATOR), SEPARATOR)?,
            None => NodeIter::new(schema, [0; Y], 0, SEPARATOR),
        };
        Ok(Self::Dump { iter })
    }

    pub(crate) fn list(
        schema: &'static Schema,
        root: &[usize],
        reply: ReplyTarget,
    ) -> Result<Self, DescendError<()>> {
        let iter = NodeIter::with_root(schema, root, SEPARATOR)?;
        Ok(Self::List { iter, reply })
    }

    pub(crate) fn dump_root(
        schema: &'static Schema,
        root: &[usize],
    ) -> Result<Self, DescendError<()>> {
        Ok(Self::Dump {
            iter: NodeIter::with_root(schema, root, SEPARATOR)?,
        })
    }
}
