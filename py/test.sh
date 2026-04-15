#!/bin/sh

set -e
set -x

python3 -m venv --clear --system-site-packages .venv
.venv/bin/python -m pip install -e py/miniconf-mqtt
exec .venv/bin/python py/test.py
