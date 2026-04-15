# `miniconf` Python Utility

Python package for interacting with `miniconf-mqtt` targets.

The client exposes:
- `get()` / `set()` for leaf values
- `list()` / `dump()` for subtree traversal
- `schema()` for static node metadata and structure
- `state()` for runtime presence/activity information

## Installation

Run `pip install .` from this directory to install the `miniconf-mqtt` package.

Alternatively, run `python -m pip install
git+https://github.com/quartiq/miniconf#subdirectory=py/miniconf-mqtt` to avoid cloning locally.
