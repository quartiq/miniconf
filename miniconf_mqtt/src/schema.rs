use heapless::String;
use miniconf::{ConstPath, NodeIter, Schema, compact_schema::SchemaDefs};

use crate::{MAX_DEPTH, MAX_SCHEMA_DEFS, MAX_TOPIC_LENGTH};

pub(crate) struct SchemaSync {
    pub(crate) defs: SchemaDefs<MAX_SCHEMA_DEFS>,
    pub(crate) next: usize,
    pub(crate) page: usize,
    pub(crate) hash: u32,
}

impl SchemaSync {
    pub(crate) fn new(schema: &'static Schema) -> Self {
        Self {
            defs: SchemaDefs::new(schema).unwrap(),
            next: 0,
            page: 0,
            hash: <u32 as yafnv::Fnv>::OFFSET_BASIS,
        }
    }
}

pub(crate) type SettingsSync = NodeIter<ConstPath<String<MAX_TOPIC_LENGTH>, '/'>, MAX_DEPTH>;
