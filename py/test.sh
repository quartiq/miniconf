#!/bin/sh

set -e
set -x

python -m venv .venv
. .venv/bin/activate
python -m pip install -e py/miniconf-mqtt

PREFIX=test

# test no residual DUTs alive
ALIVE=$(timeout --foreground 1 mosquitto_sub -t "$PREFIX/+/alive" -h localhost -F '%p' || true)
test "$ALIVE" = "" -o "$ALIVE" = "0"

# build and start DUT
cargo build -p miniconf_mqtt --example mqtt
cargo run -p miniconf_mqtt --example mqtt &
DUT_PID=$!

# check initial dump (9 settings)
# 3 > DUMP_TIMEOUT_SECONDS
DUMP=$(timeout --foreground 3 mosquitto_sub -t "$PREFIX/+/settings/#" -h localhost -F '%t' | wc -l)
test $DUMP = 9

# test alive-ness
ALIVE=$(timeout --foreground 1 mosquitto_sub -t "$PREFIX/+/alive" -h localhost -F '%p' || true)
test "$ALIVE" = "\"hello\""

# no discover SET
python -m miniconf -b localhost $PREFIX/id '/stream="192.0.2.16:9293"'
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

# test exited
($MC 2>/dev/null && exit 1) || true
# without miniconf
ALIVE=$(timeout --foreground 1 mosquitto_sub -t "$PREFIX/+/alive" -h localhost -F '%p' || true)
test "$ALIVE" = ""
