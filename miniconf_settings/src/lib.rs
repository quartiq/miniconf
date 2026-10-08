#![doc = include_str!("../README.md")]
#![no_std]

#[cfg(feature = "flash")]
pub mod flash;
#[cfg(feature = "shell")]
pub mod shell;
pub mod snapshot;

/// Maximum settings-tree depth supported by shell and snapshot traversal.
pub const MAX_DEPTH: usize = 12;

/// Maximum settings-path length in bytes for shell output and snapshots.
pub const MAX_PATH_LENGTH: usize = 128;

#[cfg(test)]
extern crate std;

#[cfg(test)]
fn init_host_logging() {
    static LOGGING: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    LOGGING.get_or_init(|| {
        env_logger::builder().is_test(true).try_init().unwrap();
        defmt2log::init_from_current_exe();
    });
}
