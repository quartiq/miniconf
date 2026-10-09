"""Run against a built terminal binary: python3 miniconf_settings/examples/test_shell.py target/debug/examples/shell."""

import os
import pty
import select
import subprocess
import sys
import termios
import time

binary = sys.argv[1]
result = subprocess.run(
    [binary],
    input="set /output/dac/0 2048\nget /output/dac/0\n",
    text=True,
    capture_output=True,
    check=True,
    timeout=30,
)
assert result.stdout == "Set.\n/output/dac/0: 2048\n"
master, slave = pty.openpty()
original = termios.tcgetattr(slave)
process = subprocess.Popen([binary], stdin=slave, stdout=slave, stderr=slave)
queries = 0
received = b""
position = 0


def expect(expected):
    global queries, received, position
    deadline = time.monotonic() + 30
    while expected not in received[position:]:
        assert time.monotonic() < deadline, received[position:]
        if not select.select([master], [], [], 0.1)[0]:
            continue
        received += os.read(master, 65536)
        while queries < received.count(b"\x1b[6n"):
            if queries == 0:
                # Reject a malformed reply; retain queued editing and a literal R.
                os.write(master, b"help\rget /output/dac/0\x1b[DR\x7f\r\x1b[;20R")
            else:
                os.write(master, b"\x1b[24;20R" if queries % 2 == 1 else b"\x1b[1;3R")
            queries += 1
    position = received.index(expected, position) + len(expected)


try:
    expect(b"Tab completes; repeat to list matches.\r\n")
    expect(b"/output/dac/0: 1024\r\n")
    expect(b"> \x1b[1;3H")
    os.write(master, b"schema /out\t\r")
    expect(b"/output [named]")
    expect(b"    0..2 [leaf] [sem ty=i16]\r\n")
    expect(b"> \x1b[1;3H")
    os.write(master, b"se\x1b\t/out\t/dac/0\t2048\r")
    expect(b"Set.\r\n")
    expect(b"> \x1b[1;3H")
    os.write(master, b"get /out\t\t\r")
    expect(b"/output/dac/0: 2048\r\n")
    expect(b"> \x1b[1;3H")
    assert received.count(b"/output [named]") == 1
    os.write(master, b"get /c\t\t\t\x03")
    expect(b"control/\r\ncalibration/\r\n")
    expect(b"control/\r\ncalibration/\r\n")
    expect(b"> \x1b[1;3H")
    # Match listings preserve a mid-line cursor and accept queued editing immediately.
    os.write(master, b"get /output/dac/1\x1b[D\t\t\t\x040\t\r")
    expect(b"0  1\r\n")
    expect(b"0  1\r\n")
    expect(b"/output/dac/0: 2048\r\n")
    assert received.count(b"0  1\r\n") == 2
    expect(b"> \x1b[1;3H")
    os.write(master, b"set /output/dac/0 99\x03get /output/dac/0\r")
    expect(b"/output/dac/0: 2048\r\n")
    expect(b"> \x1b[1;3H")
    os.write(master, b"\x1b[A\r")
    expect(b"/output/dac/0: 2048\r\n")
    expect(b"> \x1b[1;3H")
    os.write(master, b"\x04")
    assert process.wait(timeout=5) == 0
    assert termios.tcgetattr(slave) == original
finally:
    if process.poll() is None:
        process.kill()
        process.wait()
    os.close(master)
    os.close(slave)
print("Pipes, completion, match listing, and terminal restoration passed.")
