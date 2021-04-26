# MiniConf

[![QUARTIQ Matrix Chat](https://img.shields.io/matrix/quartiq:matrix.org)](https://matrix.to/#/#quartiq:matrix.org)
![Continuous Integration](https://github.com/vertigo-designs/miniconf/workflows/Continuous%20Integration/badge.svg)

MiniConf is a `no_std` minimal run-time settings configuration tool designed to be run on top of
any communication means. It was originally designed to work with MQTT clients.

# Design

Miniconf provides an easy-to-work-with API for quickly adding runtime-configured settings to any
embedded project. This allows any internet-connected device to quickly bring up configuration
interfaces with minimal implementation in the end-user application.

MiniConf provides a `Miniconf` derive macro for creating a settings structure, e.g.:
```rust
use miniconf::Miniconf;

#[derive(Miniconf)]
struct NestedSettings {
    inner: f32,
}

#[derive(Miniconf)]
struct MySettings {
    initial_value: u32,
    internal: NestedSettings,
}
```

# Settings Paths

A setting value must be configured via a specific path. Paths take the form of variable names
separated by slashes - this design follows typical MQTT topic design semantics. For example, with
the following `Settings` structure:
```
#[derive(Miniconf)]
struct Data {
    inner: f32,
}

#[derive(Miniconf)]
struct Settings {
    initial_value: u32,
    internal: Data,
}
```

We can access `Data::inner` with the path `internal/inner`.

Settings may only be updated at the terminal node. That is, you cannot configure
`<device-id>/settings/internal` directly. If this is desired, instead derive `MiniconfAtomic` on the
`struct Data` definition. In this way, all members of `struct Data` must be updated simultaneously.

# Settings Values

MiniConf relies on using [`serde`](https://github.com/serde-rs/serde) for defining a
de/serialization method for settings. Currently, MiniConf only supports serde-json de/serialization
formats, although more formats may be supported in the future.
