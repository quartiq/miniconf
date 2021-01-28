# MiniConf

[![QUARTIQ Matrix Chat](https://img.shields.io/matrix/quartiq:matrix.org)](https://matrix.to/#/#quartiq:matrix.org)
![Continuous Integration](https://github.com/vertigo-designs/miniconf/workflows/Continuous%20Integration/badge.svg)

MiniConf is a `no_std` minimal run-time settings configuration tool designed to be run on top of
[`minimq`](https://github.com/quartiq/minimq), an MQTTv5 client.

# Design

Miniconf provides an easy-to-work-with API for quickly adding MQTT telemetry and settings
configuration to any embedded project by leveraging MQTT. This allows any internet-connected device
to quickly being up a telemetry and control interface with minimal implementation in the end-user
application.

In order to support synchronization primitives, MiniConf distinguishes between "active" and "staged"
settings. After configuring a settings value, it does not take effect until the staged settings are
committed. This allows for multiple settings to be updated simultaneously.

MiniConf provides a `StringSet` derive macro for creating a settings structure, e.g.:
```rust
use miniconf::StringSet;

#[derive(StringSet)]
struct NestedSettings {
    inner: f32,
}

#[derive(StringSet)]
struct MySettings {
    initial_value: u32,
    internal: NestedSettings,
}
```

# Settings Paths

A setting value must be published to a specific MQTT topic for the client to receive it. Topics take
the form of:

```
<device-id>/settings/<path>
```

In the above example, `<device-id>` is an identifier unique to the device that implements
MiniConf-settable settings. The `<path>` field represents the settings path in the root settings
structure.

For example, given the following settings structure:
```rust
use miniconf::StringSet;

#[derive(StringSet)]
struct NestedSettings {
    inner: f32,
}

#[derive(StringSet)]
struct MySettings {
    initial_value: u32,
    internal: NestedSettings,
}
```

If `MySettings` is the root settings structure, we can set the `inner` value to 3.14 by sending the
following message over MQTT:
```
topic: <device-id>/settings/internal/inner
data: 3.14
```

In order to commit settings, a message must be published to:
```
<device-id>/settings/commit
```

# Settings Values

MiniConf relies on using [`serde`](https://github.com/serde-rs/serde) for defining a
de/serialization method for settings. Currently, MiniConf only supports serde-json de/serialization
formats, although more formats may be supported in the future.
