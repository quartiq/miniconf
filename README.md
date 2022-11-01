# MiniConf

[![QUARTIQ Matrix Chat](https://img.shields.io/matrix/quartiq:matrix.org)](https://matrix.to/#/#quartiq:matrix.org)
![Continuous Integration](https://github.com/vertigo-designs/miniconf/workflows/Continuous%20Integration/badge.svg)

Miniconf is a `no_std` minimal run-time settings configuration tool designed to be run on top of
any communication means. It was originally designed to work with MQTT clients and provides a default
implementation using [minimq](https://github.com/quartiq/minimq) as the MQTT client.

Check out the [documentation](https://docs.rs/miniconf/latest/miniconf/)  for examples and detailed
information.

# Features

Miniconf provides simple tools to bring run-time configuration up on any project. Any device that
can send and receive data can leverage Miniconf to provide run-time configuration utilities.

This crate provides a derive macro to automatically map Rust structures into a key-value
lookup tool, where keys use a string-based, path-like syntax to access and modify structure members.

Miniconf also provides an MQTT client and Python utility to quickly bring IoT and remote
configuration to your project. After running programming your device, settings updates are easily
accomplished using Python:

```sh
# Set the `sample_rate_hz` value of device with identifier `quartiq/example_device` to `10`.
python -m miniconf quartiq/example_device sample_rate_hz=10
```
