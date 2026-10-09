//! Records for ordered, forward-compatible Miniconf settings snapshots.

use core::str;

use defmt::trace;
use heapless::String;
use miniconf::{
    ConstPath, DescendError, Indices, IntoKeys, KeyError, TreeDeserializeOwned, TreeSerialize,
    ValueError, json_core,
};
#[cfg(any(feature = "flash", test))]
use miniconf::{ExactSize, NodeIter, Schema};

use crate::{MAX_DEPTH, MAX_PATH_LENGTH};

/// Suggested buffer capacity for a maximum-length path and 512 bytes of JSON.
pub const RECORD_CAPACITY: usize = 3 + MAX_PATH_LENGTH + 512;

/// Snapshot encoding or decoding failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotError {
    /// The caller-provided output buffer is too small.
    BufferTooSmall,
    /// The snapshot record is malformed or incomplete.
    InvalidFormat,
    /// A settings path could not be resolved or exceeds traversal capacity.
    Path,
    /// A live leaf could not be serialized.
    Serialize,
    /// A known snapshot leaf could not be applied.
    Deserialize,
}

/// One chunk produced by [`Encoder::encode`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg(any(feature = "flash", test))]
pub(crate) struct Chunk {
    /// Bytes written to the output buffer.
    pub(crate) bytes: usize,
    /// Leaf records written to the output buffer.
    pub(crate) records: u16,
    /// Whether all schema nodes have been consumed.
    pub(crate) done: bool,
}

/// Serialize one present leaf.
/// The output buffer determines JSON capacity, up to the record's `u16` length limit.
/// Absent leaves produce `None`.
pub fn encode_record<S>(
    settings: &S,
    indices: &[usize],
    output: &mut [u8],
) -> Result<Option<usize>, SnapshotError>
where
    S: TreeSerialize,
{
    let path = S::SCHEMA
        .transcode::<ConstPath<String<MAX_PATH_LENGTH>, '/'>>(indices)
        .map_err(|_| SnapshotError::Path)?;
    let path = path.as_ref().as_bytes();
    let value_offset = 3 + path.len();
    let mut keys = indices;
    let value_len = {
        let value = output
            .get_mut(value_offset..)
            .ok_or(SnapshotError::BufferTooSmall)?;
        match json_core::get_by_keys(settings, &mut keys, value) {
            Ok(len) => len,
            Err(miniconf::SerdeError::Value(ValueError::Absent)) => {
                return Ok(None);
            }
            Err(_) => return Err(SnapshotError::Serialize),
        }
    };
    let path_len = u8::try_from(path.len()).map_err(|_| SnapshotError::Path)?;
    let value_len = u16::try_from(value_len).map_err(|_| SnapshotError::Serialize)?;
    let record_len = value_offset + usize::from(value_len);
    let record = output
        .get_mut(..record_len)
        .ok_or(SnapshotError::BufferTooSmall)?;
    record[0] = path_len;
    record[1..3].copy_from_slice(&value_len.to_le_bytes());
    record[3..3 + path.len()].copy_from_slice(path);
    Ok(Some(record_len))
}

/// Apply one known leaf record, ignoring paths absent from the current schema.
/// Apply selector records before their dependent leaves. Records are applied
/// individually, without whole-snapshot rollback.
pub fn apply_record<S>(settings: &mut S, record: &[u8]) -> Result<(), SnapshotError>
where
    S: TreeDeserializeOwned,
{
    let (path, value) = decode_record(record)?;
    let keys = path.into_keys().map_err(|_| SnapshotError::InvalidFormat)?;
    let indices = match S::SCHEMA.transcode::<Indices<[usize; MAX_DEPTH]>>(keys) {
        Ok(indices) => indices,
        Err(DescendError::Key(KeyError::NotFound | KeyError::TooLong)) => {
            trace!("Skipping unknown settings path {=str}", path);
            return Ok(());
        }
        Err(_) => return Err(SnapshotError::Path),
    };
    let mut keys = indices.as_ref();
    json_core::set_by_keys(settings, &mut keys, value).map_err(|_| SnapshotError::Deserialize)?;
    Ok(())
}

/// Traversal and a pending record length; settings are borrowed only while encoding.
#[cfg(any(feature = "flash", test))]
pub(crate) struct Encoder {
    nodes: ExactSize<NodeIter<Indices<[usize; MAX_DEPTH]>, MAX_DEPTH>>,
    pending: usize,
}

#[cfg(any(feature = "flash", test))]
impl Encoder {
    pub(crate) fn new(schema: &'static Schema) -> Self {
        Self {
            nodes: schema.nodes(),
            pending: 0,
        }
    }

    /// Keep the same record scratch buffer between calls: it may contain a pending leaf.
    pub(crate) fn encode<S: TreeSerialize>(
        &mut self,
        settings: &S,
        output: &mut [u8],
        record: &mut [u8],
    ) -> Result<Chunk, SnapshotError> {
        let mut bytes = 0;
        let mut records = 0u16;
        loop {
            if self.pending == 0 {
                let Some(indices) = self.nodes.next() else {
                    return Ok(Chunk {
                        bytes,
                        records,
                        done: true,
                    });
                };
                let indices = indices.map_err(|_| SnapshotError::Path)?;
                let Some(len) = encode_record(settings, indices.as_ref(), record)? else {
                    continue;
                };
                self.pending = len;
            }
            let Some(target) = output.get_mut(bytes..bytes + self.pending) else {
                if bytes == 0 {
                    return Err(SnapshotError::BufferTooSmall);
                }
                return Ok(Chunk {
                    bytes,
                    records,
                    done: false,
                });
            };
            target.copy_from_slice(&record[..self.pending]);
            bytes += self.pending;
            records = records.checked_add(1).ok_or(SnapshotError::Serialize)?;
            self.pending = 0;
        }
    }
}

/// Apply all complete leaf records in a serialized chunk.
#[cfg(any(feature = "flash", test))]
pub(crate) fn apply_chunk<S>(settings: &mut S, chunk: &[u8]) -> Result<u16, SnapshotError>
where
    S: TreeDeserializeOwned,
{
    let mut offset = 0;
    let mut records = 0u16;
    while offset < chunk.len() {
        let header = chunk
            .get(offset..offset + 3)
            .ok_or(SnapshotError::InvalidFormat)?;
        let len =
            3 + usize::from(header[0]) + usize::from(u16::from_le_bytes([header[1], header[2]]));
        let record = chunk
            .get(offset..offset + len)
            .ok_or(SnapshotError::InvalidFormat)?;
        apply_record(settings, record)?;
        offset += len;
        records = records.checked_add(1).ok_or(SnapshotError::InvalidFormat)?;
    }
    Ok(records)
}

fn decode_record(record: &[u8]) -> Result<(&str, &[u8]), SnapshotError> {
    let header = record.get(..3).ok_or(SnapshotError::InvalidFormat)?;
    let path_len = usize::from(header[0]);
    let value_len = usize::from(u16::from_le_bytes([header[1], header[2]]));
    if record.len() != 3 + path_len + value_len {
        return Err(SnapshotError::InvalidFormat);
    }
    let path =
        str::from_utf8(&record[3..3 + path_len]).map_err(|_| SnapshotError::InvalidFormat)?;
    Ok((path, &record[3 + path_len..]))
}

#[cfg(test)]
mod tests {
    use miniconf::{Tree, TreeSchema, str_leaf};

    use super::{
        Encoder, RECORD_CAPACITY, SnapshotError, apply_chunk, apply_record, encode_record,
    };

    #[test]
    fn rejects_invalid_records_but_skips_unknown_paths() {
        crate::init_host_logging();
        let settings = TaggedSettings::default();
        let mut record = [0; RECORD_CAPACITY];
        let len = encode_record(&settings, &[0], &mut record)
            .unwrap()
            .unwrap();
        assert_eq!(
            apply_record(&mut TaggedSettings::default(), &record[..len - 1]),
            Err(SnapshotError::InvalidFormat)
        );
        record[3] = b'x';
        assert_eq!(
            apply_record(&mut TaggedSettings::default(), &record[..len]),
            Err(SnapshotError::InvalidFormat)
        );
        assert_eq!(
            apply_record(&mut TaggedSettings::default(), b"\x08\x01\x00/missing0"),
            Ok(())
        );
    }

    #[test]
    fn record_buffers_determine_capacity() {
        crate::init_host_logging();
        assert_eq!(encode_record(&7u8, &[], &mut [0; 4]), Ok(Some(4)));
        assert_eq!(
            encode_record(&7u8, &[], &mut [0; 3]),
            Err(SnapshotError::Serialize)
        );
        let value = heapless::String::<600>::try_from("x".repeat(600).as_str()).unwrap();
        let mut record = [0; 605];
        let len = encode_record(&value, &[], &mut record).unwrap().unwrap();
        let mut loaded = heapless::String::<600>::new();
        apply_record(&mut loaded, &record[..len]).unwrap();
        assert_eq!(loaded, value);
    }

    #[derive(Clone, Debug, PartialEq, Tree)]
    enum Choice {
        A(u32),
        B(i32),
    }

    impl Default for Choice {
        fn default() -> Self {
            Self::A(0)
        }
    }

    impl AsRef<str> for Choice {
        fn as_ref(&self) -> &str {
            match self {
                Self::A(_) => "A",
                Self::B(_) => "B",
            }
        }
    }

    impl<'a> TryFrom<&'a str> for Choice {
        type Error = ();

        fn try_from(value: &'a str) -> Result<Self, Self::Error> {
            match value {
                "A" => Ok(Self::A(0)),
                "B" => Ok(Self::B(0)),
                _ => Err(()),
            }
        }
    }

    #[derive(Clone, Debug, Default, PartialEq, Tree)]
    struct TaggedSettings {
        #[tree(rename = "typ", with = str_leaf, defer = self.choice, typ = "Choice")]
        _typ: (),
        choice: Choice,
    }

    #[test]
    fn chunked_round_trip_preserves_record_order() {
        crate::init_host_logging();
        let settings = TaggedSettings {
            _typ: (),
            choice: Choice::B(-7),
        };
        let mut loaded = TaggedSettings::default();
        let mut encoder = Encoder::new(TaggedSettings::SCHEMA);
        let mut chunk = [0; 20];
        let mut record = [0; 48];
        assert_eq!(
            encoder.encode(&settings, &mut [], &mut record).unwrap_err(),
            SnapshotError::BufferTooSmall
        );
        loop {
            let encoded = encoder.encode(&settings, &mut chunk, &mut record).unwrap();
            assert_eq!(encoded.records, 1);
            apply_chunk(&mut loaded, &chunk[..encoded.bytes]).unwrap();
            if encoded.done {
                break;
            }
        }
        assert_eq!(loaded, settings);
    }
}
