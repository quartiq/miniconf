#!/bin/sh

set -e
set -x

python -m venv .venv
. .venv/bin/activate
python -m pip install -e py/miniconf-mqtt

cargo build -p miniconf_mqtt --example mqtt
cargo run -p miniconf_mqtt --example mqtt &
sleep 3 # > REPUBLISH_TIMEOUT_SECONDS

python -m miniconf -b localhost dt/sinara/dual-iir/01-02-03-04-05-06 '/stream="192.0.2.16:9293"'
python -m miniconf -b localhost -d dt/sinara/dual-iir/+ '/afe/0'       # GET
python -m miniconf -b localhost -d dt/sinara/dual-iir/+ '/afe/0="G1"'  # SET
python -m miniconf -b localhost -d dt/sinara/dual-iir/+ '/afe/0='      # CLEAR
python -m miniconf -b localhost -d dt/sinara/dual-iir/+ '/afe?' '?'    # LIST-GET
python -m miniconf -b localhost -d dt/sinara/dual-iir/+ '/afe!'        # DUMP
sleep 1  # dump is asynchronous

python -m miniconf -b localhost -d dt/sinara/dual-iir/+ '/exit=true'
