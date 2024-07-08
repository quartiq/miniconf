#!/bin/sh

set -e
set -x

python -m venv .venv
. .venv/bin/activate
python -m pip install -e py/miniconf-mqtt

cargo build -p miniconf_mqtt --example mqtt &
cargo run -p miniconf_mqtt --example mqtt &
sleep 3 # > REPUBLISH_TIMEOUT_SECONDS

python -m miniconf -b localhost -d 'sample/+' '!' # DUMP
sleep 1  # dump is asynchronous
python -m miniconf -b localhost -d 'sample/+' '?' # LIST
python -m miniconf -b localhost -d 'sample/+' '/amplitude/0=3' '/inner/frame_rate=9' # SET
python -m miniconf -b localhost -d 'sample/+' '/array' # GET
python -m miniconf -b localhost -d 'sample/+' '/inner/frame_rate=' # CLEAR
python -m miniconf -b localhost -d 'sample/+' '/exit=true' # EXIT
