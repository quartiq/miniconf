//! Atomic, whole-tree settings storage on a `sequential-storage` map.

use defmt::{Debug2Format, debug};
use embedded_storage_async::nor_flash::NorFlash;
use miniconf::{TreeDeserializeOwned, TreeSerialize};
use sequential_storage::{
    cache::{Cache, Uncached},
    map::{MapConfig, MapStorage},
};

use crate::snapshot::{self, SnapshotError};

const SLOT_COUNT: usize = 2;
// Two full 64-chunk slots plus one replacement item fit in a 128 KiB sector.
const MAX_CHUNKS: usize = 64;
const CHUNK_DATA_CAPACITY: usize = 672;
const CHUNK_HEADER_LEN: usize = 5;
const CHUNK_VALUE_CAPACITY: usize = CHUNK_HEADER_LEN + CHUNK_DATA_CAPACITY;
const MAP_BUFFER_CAPACITY: usize = (size_of::<u16>() + CHUNK_VALUE_CAPACITY).next_multiple_of(32);
const MANIFEST_KEY: [u16; SLOT_COUNT] = [0, 1];
const MANIFEST_MAGIC: &[u8; 4] = b"MCS\x03";
const MANIFEST_LEN: usize = 16;

const _: () = assert!(CHUNK_DATA_CAPACITY >= snapshot::RECORD_CAPACITY);

/// Scratch memory used while loading or storing settings.
#[repr(C, align(32))]
pub struct Buffers {
    map: [u8; MAP_BUFFER_CAPACITY],
    chunk: [u8; CHUNK_VALUE_CAPACITY],
    record: [u8; snapshot::RECORD_CAPACITY],
}

impl Buffers {
    /// Create zeroed settings-storage scratch buffers.
    pub const fn new() -> Self {
        Self {
            map: [0; MAP_BUFFER_CAPACITY],
            chunk: [0; CHUNK_VALUE_CAPACITY],
            record: [0; snapshot::RECORD_CAPACITY],
        }
    }
}

impl Default for Buffers {
    fn default() -> Self {
        Self::new()
    }
}

/// Persistent settings operation failure.
#[derive(Debug)]
pub enum Error<E> {
    /// A settings leaf could not be encoded or decoded.
    Snapshot(SnapshotError),
    /// The flash map operation failed.
    Storage(sequential_storage::Error<E>),
    /// The settings tree exceeds the fixed snapshot capacity.
    TooLarge,
    /// The persistent generation counter is exhausted.
    GenerationExhausted,
    /// Neither [`Store::load`] nor [`Store::erase`] has completed successfully.
    NotLoaded,
    /// No stored snapshot or defaults marker could be accepted.
    NoSnapshot,
}

/// Persistent state selected during boot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    /// A complete settings snapshot was loaded.
    Stored,
    /// The caller-provided firmware defaults remain active.
    Defaults,
}

/// Result of selecting persistent state during boot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Load {
    /// Selected persistent state.
    pub state: State,
    /// A newer invalid snapshot or malformed manifest was skipped.
    pub recovered: bool,
}

/// Current persistent settings state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Status {
    /// Selected state, or `None` when flash did not load successfully.
    pub state: Option<State>,
    /// Generation of the selected state.
    pub generation: Option<u32>,
    /// Whether loading discarded invalid persistent state.
    pub recovered: bool,
}

impl<E> From<SnapshotError> for Error<E> {
    fn from(error: SnapshotError) -> Self {
        Self::Snapshot(error)
    }
}

impl<E> From<sequential_storage::Error<E>> for Error<E> {
    fn from(error: sequential_storage::Error<E>) -> Self {
        Self::Storage(error)
    }
}

/// Atomic settings store backed by one map spanning at least two erase pages.
pub struct Store<'a, S: NorFlash> {
    map: MapStorage<u16, S, Cache<Uncached, Uncached, Uncached, u16>>,
    buffers: Scratch<'a>,
    // Reserve entries before writing so cancellation cannot reuse their generation.
    head: Option<Entry>,
    active: Option<Entry>,
    recovered: bool,
}

struct Scratch<'a> {
    map: &'a mut [u8],
    chunk: &'a mut [u8],
    record: &'a mut [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Entry {
    slot: usize,
    generation: u32,
    snapshot: Snapshot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Snapshot {
    Stored(Description),
    Defaults,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Description {
    chunks: u8,
    records: u16,
    bytes: u32,
}

impl<'a, S: NorFlash> Store<'a, S> {
    /// Open a map over the entire flash object.
    pub fn new(flash: S, buffers: &'a mut Buffers) -> Self {
        Self::with_buffers(
            flash,
            &mut buffers.map,
            &mut buffers.chunk,
            &mut buffers.record,
        )
    }

    /// Open a map with caller-sized scratch memory.
    ///
    /// `chunk` includes a five-byte header; each encoded leaf must fit in its remainder.
    /// `record` determines leaf JSON capacity. `map` must hold the largest key-value
    /// item, including items already stored, rounded up to flash word size. Its RAM
    /// alignment must also satisfy the flash driver.
    /// Smaller chunk buffers need more of the format's fixed 64 chunks per snapshot.
    ///
    /// Panics if the chunk header, or a chunk or manifest plus key, cannot fit.
    pub fn with_buffers(
        flash: S,
        map: &'a mut [u8],
        chunk: &'a mut [u8],
        record: &'a mut [u8],
    ) -> Self {
        assert!(chunk.len() >= CHUNK_HEADER_LEN, "chunk buffer too small");
        let word_size = S::READ_SIZE.max(S::WRITE_SIZE);
        assert!(
            map.len()
                >= (size_of::<u16>() + chunk.len().max(MANIFEST_LEN)).next_multiple_of(word_size),
            "map buffer too small"
        );
        let end = u32::try_from(flash.capacity()).expect("settings flash capacity exceeds u32");
        Self {
            map: MapStorage::new(flash, MapConfig::new(0..end), Cache::new_uncached()),
            buffers: Scratch { map, chunk, record },
            head: None,
            active: None,
            recovered: false,
        }
    }

    /// Report the last successfully selected or committed state.
    pub fn status(&self) -> Status {
        Status {
            state: self.active.map(|entry| match entry.snapshot {
                Snapshot::Stored(_) => State::Stored,
                Snapshot::Defaults => State::Defaults,
            }),
            generation: self.active.map(|entry| entry.generation),
            recovered: self.recovered,
        }
    }

    /// Load the newest valid snapshot, falling back to the other logical slot.
    ///
    /// Failure leaves `settings` unchanged when clones have independent mutable state.
    /// Setter side effects are not rolled back. Erasing unusable storage is explicit.
    /// The underlying map may repair interrupted writes while reading.
    pub fn load<'s, T>(
        &'s mut self,
        settings: &'s mut T,
    ) -> impl core::future::Future<Output = Result<Load, Error<S::Error>>> + 's
    where
        T: TreeDeserializeOwned + Clone,
    {
        self.load_validated(settings, |_| true)
    }

    /// Load the newest intact snapshot accepted by the application.
    /// Caller-provided defaults are trusted; rejected snapshots leave them unchanged.
    /// Clones must have independent mutable settings state. Setter and validation
    /// side effects are not rolled back.
    pub async fn load_validated<T>(
        &mut self,
        settings: &mut T,
        valid: impl Fn(&T) -> bool,
    ) -> Result<Load, Error<S::Error>>
    where
        T: TreeDeserializeOwned + Clone,
    {
        self.active = None;
        self.recovered = false;
        let (entries, malformed) = self.scan().await?;
        let [first, second] = newest_first(entries);
        let mut recovered = malformed;
        for entry in [first, second].into_iter().flatten() {
            match entry.snapshot {
                Snapshot::Defaults => {
                    debug!("Selected settings defaults generation={}", entry.generation);
                    self.active = Some(entry);
                    self.recovered = recovered;
                    return Ok(Load {
                        state: State::Defaults,
                        recovered,
                    });
                }
                Snapshot::Stored(description) => {
                    let mut candidate = settings.clone();
                    match self.load_snapshot(&mut candidate, entry, description).await {
                        Ok(()) if valid(&candidate) => {
                            debug!(
                                "Loaded settings snapshot generation={} slot={}",
                                entry.generation, entry.slot
                            );
                            *settings = candidate;
                            self.active = Some(entry);
                            self.recovered = recovered;
                            return Ok(Load {
                                state: State::Stored,
                                recovered,
                            });
                        }
                        Ok(()) => {
                            debug!(
                                "Application rejected settings snapshot generation={}",
                                entry.generation
                            );
                            recovered = true;
                        }
                        Err(Error::Snapshot(error)) => {
                            debug!(
                                "Invalid settings snapshot generation={}: {}",
                                entry.generation,
                                Debug2Format(&error)
                            );
                            recovered = true;
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
        }
        Err(Error::NoSnapshot)
    }

    /// Persist the complete current settings tree.
    /// Persisted leaves must be writable, with selectors preceding their
    /// dependent leaves in tree order.
    pub async fn store<T>(&mut self, settings: &T) -> Result<(), Error<S::Error>>
    where
        T: TreeSerialize,
    {
        let active = self.active.ok_or(Error::NotLoaded)?;
        let description = self.describe(settings)?;
        if !self.recovered
            && self.active == self.head
            && active.snapshot == Snapshot::Stored(description)
            && self.matches(settings, active, description).await?
        {
            return Ok(());
        }

        let entry = Entry {
            slot: 1 - active.slot,
            generation: next_generation(self.head)?,
            snapshot: Snapshot::Stored(description),
        };
        self.head = Some(entry);
        self.write_snapshot(settings, entry, description).await?;
        self.publish(entry).await?;
        self.recovered = false;
        Ok(())
    }

    /// Make future boots use the firmware defaults without changing current settings.
    pub async fn reset(&mut self) -> Result<(), Error<S::Error>> {
        let active = self.active.ok_or(Error::NotLoaded)?;
        if !self.recovered && self.active == self.head && active.snapshot == Snapshot::Defaults {
            return Ok(());
        }
        let entry = Entry {
            slot: 1 - active.slot,
            generation: next_generation(self.head)?,
            snapshot: Snapshot::Defaults,
        };
        self.head = Some(entry);
        self.publish(entry).await?;
        self.recovered = false;
        Ok(())
    }

    /// Recover the underlying flash object.
    pub fn into_flash(self) -> S {
        self.map.destroy().0
    }

    /// Erase the entire map and select caller-owned defaults for subsequent boots.
    /// This also prepares blank storage for its first snapshot.
    pub async fn erase(&mut self) -> Result<(), Error<S::Error>> {
        debug!("Erasing settings storage");
        self.head = None;
        self.active = None;
        self.recovered = false;
        self.map.erase_all().await?;
        self.publish(Entry {
            slot: 0,
            generation: 0,
            snapshot: Snapshot::Defaults,
        })
        .await
    }

    async fn scan(&mut self) -> Result<([Option<Entry>; SLOT_COUNT], bool), Error<S::Error>> {
        let mut entries = [None; SLOT_COUNT];
        let mut malformed = false;
        for (slot, entry) in entries.iter_mut().enumerate() {
            let Some(raw) = self
                .map
                .fetch_item::<&[u8]>(self.buffers.map, &MANIFEST_KEY[slot])
                .await?
            else {
                continue;
            };
            *entry = parse_manifest(slot, raw);
            malformed |= entry.is_none();
        }
        self.head = newest_first(entries)[0];
        Ok((entries, malformed))
    }

    fn describe<T>(&mut self, settings: &T) -> Result<Description, Error<S::Error>>
    where
        T: TreeSerialize,
    {
        let mut encoder = snapshot::Encoder::new(T::SCHEMA);
        let mut chunks = 0usize;
        let mut records = 0u16;
        let mut bytes = 0u32;
        loop {
            let chunk = encoder.encode(
                settings,
                &mut self.buffers.chunk[CHUNK_HEADER_LEN..],
                self.buffers.record,
            )?;
            if chunk.bytes != 0 {
                chunks += 1;
                if chunks > MAX_CHUNKS {
                    return Err(Error::TooLarge);
                }
                records = records.checked_add(chunk.records).ok_or(Error::TooLarge)?;
                bytes = bytes
                    .checked_add(u32::try_from(chunk.bytes).map_err(|_| Error::TooLarge)?)
                    .ok_or(Error::TooLarge)?;
            }
            if chunk.done {
                return Ok(Description {
                    chunks: chunks as u8,
                    records,
                    bytes,
                });
            }
        }
    }

    async fn matches<T>(
        &mut self,
        settings: &T,
        entry: Entry,
        description: Description,
    ) -> Result<bool, Error<S::Error>>
    where
        T: TreeSerialize,
    {
        let mut encoder = snapshot::Encoder::new(T::SCHEMA);
        for index in 0..description.chunks {
            let chunk = encoder.encode(
                settings,
                &mut self.buffers.chunk[CHUNK_HEADER_LEN..],
                self.buffers.record,
            )?;
            write_chunk_header(self.buffers.chunk, entry.generation, index);
            let expected = &self.buffers.chunk[..CHUNK_HEADER_LEN + chunk.bytes];
            let Some(stored) = self
                .map
                .fetch_item::<&[u8]>(self.buffers.map, &chunk_key(entry.slot, index))
                .await?
            else {
                return Ok(false);
            };
            if stored != expected {
                return Ok(false);
            }
        }
        Ok(true)
    }

    async fn write_snapshot<T>(
        &mut self,
        settings: &T,
        entry: Entry,
        description: Description,
    ) -> Result<(), Error<S::Error>>
    where
        T: TreeSerialize,
    {
        let mut encoder = snapshot::Encoder::new(T::SCHEMA);
        for index in 0..description.chunks {
            let chunk = encoder.encode(
                settings,
                &mut self.buffers.chunk[CHUNK_HEADER_LEN..],
                self.buffers.record,
            )?;
            write_chunk_header(self.buffers.chunk, entry.generation, index);
            self.map
                .store_item(
                    self.buffers.map,
                    &chunk_key(entry.slot, index),
                    &&self.buffers.chunk[..CHUNK_HEADER_LEN + chunk.bytes],
                )
                .await?;
        }
        Ok(())
    }

    async fn publish(&mut self, entry: Entry) -> Result<(), Error<S::Error>> {
        let manifest = manifest(entry);
        let result = self
            .map
            .store_item(
                self.buffers.map,
                &MANIFEST_KEY[entry.slot],
                &manifest.as_slice(),
            )
            .await;
        if let Err(error) = result {
            if self.scan().await.is_ok_and(|_| self.head == Some(entry)) {
                self.active = Some(entry);
                return Ok(());
            }
            return Err(error.into());
        }
        self.head = Some(entry);
        self.active = Some(entry);
        debug!(
            "Committed settings generation={} slot={}",
            entry.generation, entry.slot
        );
        Ok(())
    }

    async fn load_snapshot<T>(
        &mut self,
        settings: &mut T,
        entry: Entry,
        description: Description,
    ) -> Result<(), Error<S::Error>>
    where
        T: TreeDeserializeOwned,
    {
        let mut records = 0u16;
        let mut bytes = 0u32;
        for index in 0..description.chunks {
            let raw = self
                .map
                .fetch_item::<&[u8]>(self.buffers.map, &chunk_key(entry.slot, index))
                .await?
                .ok_or(SnapshotError::InvalidFormat)?;
            let data = parse_chunk(raw, entry.generation, index)?;
            records = records
                .checked_add(snapshot::apply_chunk(settings, data)?)
                .ok_or(SnapshotError::InvalidFormat)?;
            bytes = bytes
                .checked_add(u32::try_from(data.len()).map_err(|_| Error::TooLarge)?)
                .ok_or(Error::TooLarge)?;
        }
        if records != description.records || bytes != description.bytes {
            return Err(SnapshotError::InvalidFormat.into());
        }
        Ok(())
    }
}

fn chunk_key(slot: usize, index: u8) -> u16 {
    2 + (slot * MAX_CHUNKS + usize::from(index)) as u16
}

fn write_chunk_header(chunk: &mut [u8], generation: u32, index: u8) {
    chunk[..4].copy_from_slice(&generation.to_le_bytes());
    chunk[4] = index;
}

fn parse_chunk(chunk: &[u8], generation: u32, index: u8) -> Result<&[u8], SnapshotError> {
    if chunk.len() < CHUNK_HEADER_LEN || chunk[..4] != generation.to_le_bytes() || chunk[4] != index
    {
        return Err(SnapshotError::InvalidFormat);
    }
    Ok(&chunk[CHUNK_HEADER_LEN..])
}

fn manifest(entry: Entry) -> [u8; MANIFEST_LEN] {
    let mut manifest = [0; MANIFEST_LEN];
    manifest[..4].copy_from_slice(MANIFEST_MAGIC);
    manifest[4] = match entry.snapshot {
        Snapshot::Stored(_) => 1,
        Snapshot::Defaults => 2,
    };
    manifest[5..9].copy_from_slice(&entry.generation.to_le_bytes());
    if let Snapshot::Stored(description) = entry.snapshot {
        manifest[9] = description.chunks;
        manifest[10..12].copy_from_slice(&description.records.to_le_bytes());
        manifest[12..16].copy_from_slice(&description.bytes.to_le_bytes());
    }
    manifest
}

fn parse_manifest(slot: usize, manifest: &[u8]) -> Option<Entry> {
    if manifest.len() != MANIFEST_LEN || manifest.get(..4)? != MANIFEST_MAGIC {
        return None;
    }
    let generation = u32::from_le_bytes(manifest[5..9].try_into().ok()?);
    let snapshot = match manifest[4] {
        1 => {
            let chunks = manifest[9];
            if usize::from(chunks) > MAX_CHUNKS {
                return None;
            }
            Snapshot::Stored(Description {
                chunks,
                records: u16::from_le_bytes(manifest[10..12].try_into().ok()?),
                bytes: u32::from_le_bytes(manifest[12..16].try_into().ok()?),
            })
        }
        2 if manifest[9..].iter().all(|byte| *byte == 0) => Snapshot::Defaults,
        _ => return None,
    };
    Some(Entry {
        slot,
        generation,
        snapshot,
    })
}

fn newest_first(entries: [Option<Entry>; SLOT_COUNT]) -> [Option<Entry>; SLOT_COUNT] {
    let [a, b] = entries;
    if a.map(|entry| entry.generation) < b.map(|entry| entry.generation) {
        [b, a]
    } else {
        [a, b]
    }
}

fn next_generation<E>(head: Option<Entry>) -> Result<u32, Error<E>> {
    head.map_or(Ok(0), |entry| {
        entry
            .generation
            .checked_add(1)
            .ok_or(Error::GenerationExhausted)
    })
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;
    use miniconf::Tree;
    use sequential_storage::{
        cache::Cache,
        map::{MapConfig, MapStorage},
        mock_flash::{MockFlashBase, WriteCountCheck},
        queue::{QueueConfig, QueueStorage},
    };

    use super::{Buffers, Error, State, Store};

    const PAGE_SIZE: usize = 4096;
    type Flash = MockFlashBase<2, 32, { PAGE_SIZE / 32 }>;

    fn flash() -> Flash {
        crate::init_host_logging();
        Flash::new(WriteCountCheck::OnceOnly, None, true)
    }

    #[derive(Clone, Debug, Default, PartialEq, Tree)]
    struct Number {
        value: u32,
    }

    #[derive(Clone, Debug, Default, Tree)]
    struct Boolean {
        value: bool,
    }

    #[test]
    fn cancelled_commit_does_not_reuse_generation() {
        use embedded_storage_async::nor_flash::{ErrorType, NorFlash, ReadNorFlash};

        struct Paused {
            flash: Flash,
            manifest: bool,
        }
        impl ErrorType for Paused {
            type Error = <Flash as ErrorType>::Error;
        }
        impl ReadNorFlash for Paused {
            const READ_SIZE: usize = Flash::READ_SIZE;
            async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
                self.flash.read(offset, bytes).await
            }
            fn capacity(&self) -> usize {
                self.flash.capacity()
            }
        }
        impl NorFlash for Paused {
            const WRITE_SIZE: usize = Flash::WRITE_SIZE;
            const ERASE_SIZE: usize = Flash::ERASE_SIZE;
            async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
                self.flash.erase(from, to).await
            }
            async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
                self.flash.write(offset, bytes).await?;
                if self.manifest && bytes.windows(4).any(|w| w == super::MANIFEST_MAGIC) {
                    self.manifest = false;
                    futures::future::pending::<()>().await;
                }
                Ok(())
            }
        }

        block_on(async {
            let mut buffers = Buffers::new();
            let mut store = Store::new(
                Paused {
                    flash: flash(),
                    manifest: false,
                },
                &mut buffers,
            );
            let original = [1_111_111_111u32; 48];
            store.erase().await.unwrap();
            store.store(&original).await.unwrap();
            store.map.flash().manifest = true;
            let next = [2_222_222_222u32; 48];
            assert!(futures::poll!(std::pin::pin!(store.store(&next))).is_pending());
            // Interrupt the retry after its first chunk, leaving the other chunk intact.
            store.map.flash().flash.bytes_until_shutoff = Some(800);
            assert!(store.store(&[3_333_333_333u32; 48]).await.is_err());
            let mut flash = store.into_flash();
            flash.flash.bytes_until_shutoff = None;
            let mut store = Store::new(flash, &mut buffers);
            let mut loaded = [0u32; 48];
            store.load(&mut loaded).await.unwrap();
            assert_eq!(loaded, original);
        });
    }

    #[test]
    fn application_validation_tries_older_snapshot() {
        block_on(async {
            let mut buffers = Buffers::new();
            let mut store = Store::new(flash(), &mut buffers);
            store.erase().await.unwrap();
            store.store(&Number { value: 7 }).await.unwrap();
            store.store(&Number { value: 99 }).await.unwrap();
            let mut settings = Number::default();
            assert!(
                store
                    .load_validated(&mut settings, |s| s.value < 10)
                    .await
                    .unwrap()
                    .recovered
            );
            assert_eq!(settings.value, 7);
            assert!(matches!(
                store.load_validated(&mut settings, |_| false).await,
                Err(Error::NoSnapshot)
            ));
            assert_eq!(settings.value, 7);
        });
    }

    #[test]
    #[should_panic(expected = "map buffer too small")]
    fn map_buffer_must_fit_the_manifest() {
        let flash =
            MockFlashBase::<2, 4, { PAGE_SIZE / 4 }>::new(WriteCountCheck::OnceOnly, None, true);
        Store::with_buffers(flash, &mut [0; 12], &mut [0; 9], &mut [0; 4]);
    }

    #[test]
    fn caller_sized_buffers_load_with_default_buffers() {
        block_on(async {
            let mut buffers = Buffers::new();
            let mut store = Store::with_buffers(
                flash(),
                &mut buffers.map,
                &mut buffers.chunk[..25],
                &mut buffers.record[..24],
            );
            let settings = [17u32; 12];
            store.erase().await.unwrap();
            store.store(&settings).await.unwrap();
            let mut store = Store::new(store.into_flash(), &mut buffers);
            let mut loaded = [0u32; 12];
            store.load(&mut loaded).await.unwrap();
            assert_eq!(loaded, settings);
        });
    }

    #[test]
    fn round_trip_compaction_reset_and_exact_noop() {
        block_on(async {
            let mut buffers = Buffers::new();
            let mut flash = flash();
            let mut store = Store::new(flash, &mut buffers);
            assert!(matches!(
                store.store(&Number::default()).await,
                Err(Error::NotLoaded)
            ));
            flash = store.into_flash();
            let mut store = Store::new(flash, &mut buffers);
            assert!(matches!(
                store.load(&mut Number::default()).await,
                Err(Error::NoSnapshot)
            ));
            store.erase().await.unwrap();
            flash = store.into_flash();
            for value in 0..40 {
                let mut store = Store::new(flash, &mut buffers);
                store.load(&mut Number::default()).await.unwrap();
                let settings = Number { value };
                store.store(&settings).await.unwrap();
                flash = store.into_flash();

                let mut store = Store::new(flash, &mut buffers);
                let mut loaded = Number::default();
                let load = store.load(&mut loaded).await.unwrap();
                assert!(!load.recovered);
                assert_eq!(loaded, settings);
                flash = store.into_flash();
            }

            let before = flash.stats_snapshot();
            let mut store = Store::new(flash, &mut buffers);
            let mut loaded = Number::default();
            store.load(&mut loaded).await.unwrap();
            store.store(&Number { value: 39 }).await.unwrap();
            flash = store.into_flash();
            assert_eq!(before.compare_to(flash.stats_snapshot()).writes, 0);

            let mut store = Store::new(flash, &mut buffers);
            store.load(&mut Number::default()).await.unwrap();
            store.reset().await.unwrap();
            flash = store.into_flash();
            let mut store = Store::new(flash, &mut buffers);
            let mut loaded = Number { value: 123 };
            store.load(&mut loaded).await.unwrap();
            assert_eq!(loaded, Number { value: 123 });
            assert_eq!(store.status().state, Some(State::Defaults));
        });
    }

    #[test]
    fn invalid_newest_snapshot_falls_back_without_reusing_its_generation() {
        block_on(async {
            let mut buffers = Buffers::new();
            let mut store = Store::new(flash(), &mut buffers);
            store.erase().await.unwrap();
            store.store(&Number { value: 7 }).await.unwrap();
            store.store(&Boolean { value: true }).await.unwrap();
            let flash = store.into_flash();

            let mut store = Store::new(flash, &mut buffers);
            let mut loaded = Number::default();
            let load = store.load(&mut loaded).await.unwrap();
            assert!(load.recovered);
            assert_eq!(loaded.value, 7);
            assert_eq!(store.status().generation, Some(1));
            assert!(store.status().recovered);

            store.store(&Number { value: 9 }).await.unwrap();
            assert_eq!(store.status().generation, Some(3));
            assert!(!store.status().recovered);
            let flash = store.into_flash();

            let mut store = Store::new(flash, &mut buffers);
            let mut loaded = Number::default();
            store.load(&mut loaded).await.unwrap();
            assert_eq!(loaded.value, 9);
            assert_eq!(store.status().generation, Some(3));

            let flash = store.into_flash();
            let before = flash.stats_snapshot();
            let mut store = Store::new(flash, &mut buffers);
            let mut incompatible = Boolean { value: true };
            assert!(matches!(
                store.load(&mut incompatible).await,
                Err(Error::NoSnapshot)
            ));
            assert!(incompatible.value);
            assert_eq!(store.status().state, None);
            let changes = before.compare_to(store.into_flash().stats_snapshot());
            assert_eq!((changes.writes, changes.erases), (0, 0));
        });
    }

    #[test]
    fn interrupted_commit_exposes_only_the_old_or_new_snapshot() {
        block_on(async {
            let mut buffers = Buffers::new();
            let mut store = Store::new(flash(), &mut buffers);
            store.erase().await.unwrap();
            store.store(&Number { value: 7 }).await.unwrap();
            let base = store.into_flash();

            let mut completed = false;
            for cutoff in 0..512 {
                let mut interrupted = base.clone();
                interrupted.bytes_until_shutoff = Some(cutoff);
                let mut store = Store::new(interrupted, &mut buffers);
                let mut loaded = Number::default();
                store.load(&mut loaded).await.unwrap();
                let result = store.store(&Number { value: 9 }).await;
                let mut interrupted = store.into_flash();
                interrupted.bytes_until_shutoff = None;

                let mut store = Store::new(interrupted, &mut buffers);
                store.load(&mut loaded).await.unwrap();
                assert!(loaded.value == 7 || loaded.value == 9);
                if result.is_ok() {
                    completed = true;
                    break;
                }
            }
            assert!(completed);
        });
    }

    #[test]
    fn unknown_queue_format_requires_explicit_erasure() {
        block_on(async {
            let mut flash = flash();
            for slot in 0..2 {
                let start = (slot * PAGE_SIZE) as u32;
                let mut queue = QueueStorage::new(
                    &mut flash,
                    QueueConfig::new(start..start + PAGE_SIZE as u32),
                    Cache::new_uncached(),
                );
                queue.push(b"legacy queue record", false).await.unwrap();
            }

            let mut buffers = Buffers::new();
            let mut store = Store::new(flash, &mut buffers);
            let mut loaded = Number::default();
            assert!(store.load(&mut loaded).await.is_err());
            assert_eq!(store.status().state, None);
            store.erase().await.unwrap();
            store.store(&Number { value: 11 }).await.unwrap();
            let flash = store.into_flash();

            let mut store = Store::new(flash, &mut buffers);
            store.load(&mut loaded).await.unwrap();
            assert_eq!(loaded.value, 11);
        });
    }

    #[test]
    fn malformed_manifest_is_reported_and_repaired() {
        block_on(async {
            let mut buffers = Buffers::new();
            let mut store = Store::new(flash(), &mut buffers);
            store.erase().await.unwrap();
            store.store(&Number { value: 7 }).await.unwrap();
            let flash = store.into_flash();

            let mut map = MapStorage::new(
                flash,
                MapConfig::new(0..(2 * PAGE_SIZE) as u32),
                Cache::new_uncached(),
            );
            map.store_item(&mut [0; 704], &0, &&b"bad"[..])
                .await
                .unwrap();
            let flash = map.destroy().0;

            let mut store = Store::new(flash, &mut buffers);
            let mut loaded = Number::default();
            let load = store.load(&mut loaded).await.unwrap();
            assert_eq!(loaded.value, 7);
            assert!(load.recovered);
            assert!(store.status().recovered);
            store.store(&Number::default()).await.unwrap();
            assert_eq!(store.status().generation, Some(2));
            assert!(!store.status().recovered);
            let mut store = Store::new(store.into_flash(), &mut buffers);
            store.load(&mut loaded).await.unwrap();
            assert_eq!(loaded.value, 0);
        });
    }
}
