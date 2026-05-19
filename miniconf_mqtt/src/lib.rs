#![no_std]
#![warn(missing_docs)]

//! Serve a `miniconf` tree over MQTT.
//!
//! The MQTT [`minimq::Session`] is caller-owned. This crate owns only Miniconf MQTT protocol
//! state; the wire protocol and topic layout are documented in the package README.
//!
//! # Simple service
//!
//! Use [`Miniconf::startup`] once after each MQTT connection event, then call [`Miniconf::serve`]
//! in steady state.
//!
//! ```rust,no_run
//! use miniconf::Tree;
//! use miniconf_mqtt::{Error, Event, Miniconf, minimq};
//!
//! #[derive(Default, Tree)]
//! struct Settings {
//!     enabled: bool,
//!     gain: u16,
//! }
//!
//! async fn serve<IO>(
//!     miniconf: &mut Miniconf<Settings>,
//!     session: &mut minimq::Session<'_, IO>,
//!     settings: &mut Settings,
//!     event: minimq::ConnectEvent,
//! ) -> Result<(), Error<IO::Error>>
//! where
//!     IO: minimq::Io,
//! {
//!     miniconf.startup(session, settings, event).await?;
//!
//!     loop {
//!         match miniconf.serve(session, settings, |_| ()).await? {
//!             Event::Changed(path) => {
//!                 // `path` is the changed leaf's index path.
//!                 let _ = path;
//!             }
//!             Event::Unhandled(()) => {}
//!         }
//!     }
//! }
//! ```
//!
//! The `serve` callback is synchronous and runs at most once. Return copied or otherwise owned
//! application data through [`Event::Unhandled`] when unhandled traffic needs async follow-up work.
//!
//! Use [`LoadRetained`], [`Startup`], [`Service`], and [`Publisher`] directly when an application
//! must recover retained settings, bound queued protocol follow-up work, or preserve unrelated
//! inbound publishes.
//!
//! Limitations:
//! - one MQTT prefix is expected to have one authoritative device publisher
//! - retained manifest, schema, and settings publication is incremental rather than atomic

mod client;
mod message;
mod schema;
#[cfg(test)]
mod tests;

pub use minimq;

pub use client::{
    ChangedKey, Error, Event, LoadRetained, Miniconf, Publisher, Service, ServiceEvent, Startup,
};

pub(crate) use defmt::{debug, info, warn};

/// Maximum path-state depth supported by `miniconf_mqtt`.
pub const MAX_DEPTH: usize = 12;

pub(crate) const MAX_TOPIC_LENGTH: usize = minimq::TOPIC_CAPACITY;
pub(crate) const RESPONSE_CORRELATION_LENGTH: usize = 32;
pub(crate) const RESPONSE_TEXT_LENGTH: usize = 96;
pub(crate) const MAX_SCHEMA_DEFS: usize = 64;
pub(crate) const MM2_PROTO: u8 = 1;
// Expire transient request/reply traffic. Retained alive/schema/settings publications are storage.
pub(crate) const TRANSIENT_EXPIRY_SECS: u32 = 30;
pub(crate) const RETAINED_TEXT_PROPERTIES: &[minimq::Property<'static>] =
    &[minimq::Property::PayloadFormatIndicator(1)];
pub(crate) const TRANSIENT_TEXT_PROPERTIES: &[minimq::Property<'static>] = &[
    minimq::Property::PayloadFormatIndicator(1),
    minimq::Property::MessageExpiryInterval(TRANSIENT_EXPIRY_SECS),
];

/// Payload serialization failed because the provided scratch buffer was too small.
pub(crate) type EncodeError<E> = (bool, E);
