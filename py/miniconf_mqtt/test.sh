#!/bin/sh

set -e
set -x

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
IMAGE=${MINICONF_PY_IMAGE:-miniconf-py}
EXAMPLE=${MINICONF_EXAMPLE:-/work/target/debug/examples/miniconf}

cd "$ROOT"
~/.cargo/bin/cargo build -p miniconf_mqtt --example miniconf
docker build -t "$IMAGE" "$ROOT/py/miniconf_mqtt"
docker run --rm --network host \
    -v "$ROOT:/work" \
    -w /work \
    -e BROKER="${BROKER:-localhost}" \
    -e MINICONF_EXAMPLE="$EXAMPLE" \
    "$IMAGE" \
    python -I py/miniconf_mqtt/test.py
