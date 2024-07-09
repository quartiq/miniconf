#!/bin/sh

set -e
set -x

python -m venv .venv
. .venv/bin/activate
python -m pip install -e py/miniconf-mqtt

# build and start DUT
cargo build -p miniconf_mqtt --example mqtt
cargo run -p miniconf_mqtt --example mqtt &
DUT_PID=$!

PREFIX=dt/sinara/dual-iir

# check republishcation dump (1 alive + 10 settings)
# 3 > REPUBLISH_TIMEOUT_SECONDS
REPUB=$(timeout --foreground 3 mosquitto_sub -t "$PREFIX/#" -v -h localhost | wc -l)
test $REPUB -ne 11 && exit 1

# no discover SET
python -m miniconf -b localhost $PREFIX/01-02-03-04-05-06 '/stream="192.0.2.16:9293"'
# discover miniconf command
MC="python -m miniconf -b localhost -d $PREFIX/+"
# GET SET CLEAR LIST DUMP
$MC '/afe/0' '/afe/0="G10"' '/afe/0=' '/afe?' '?' '/afe!'
sleep 1  # DUMP is asynchronous

# validation ok
$MC '/four=5'
# validation error
$MC '/four=2' && exit 1

# request exit
$MC '/exit=true'
wait $DUT_PID
