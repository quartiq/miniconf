#!/bin/sh

set -e
set -x
trap 'jobs -p | xargs -r kill' EXIT

python -m venv .venv
. .venv/bin/activate
python -m pip install -e py/miniconf-mqtt

cargo build -p miniconf_mqtt --example mqtt
cargo run -p miniconf_mqtt --example mqtt &
sleep 3 # > REPUBLISH_TIMEOUT_SECONDS

MC="python -m miniconf -b localhost -d dt/sinara/dual-iir/+"

python -m miniconf -b localhost dt/sinara/dual-iir/01-02-03-04-05-06 '/stream="192.0.2.16:9293"'
# GET SET CLEAR LIST DUMP
$MC '/afe/0' '/afe/0="G10"' '/afe/0=' '/afe?' '?' '/afe!'
sleep 1  # DUMP is asynchronous

$MC '/four=5'
set +e
$MC '/four=2'
test $? -ne 1 && exit 1
set -e

$MC '/exit=true'
