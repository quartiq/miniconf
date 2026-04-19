#!/bin/sh

set -e
set -x

test -d .venv || python3 -m venv --system-site-packages .venv
.venv/bin/python -m pip install -e './py[dev]'
.venv/bin/python py/test.py
