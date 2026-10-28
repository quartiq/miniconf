"""CLI output, error handling, and bootstrap."""

import subprocess
import sys
import tempfile
import asyncio
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from pathlib import Path
from unittest import TestCase
from unittest.mock import AsyncMock, patch

from miniconf_mqtt import cli, operations
from miniconf_mqtt.errors import MiniconfException


class CliTests(TestCase):
    def test_handled_errors(self):
        async def rejected():
            async with asyncio.TaskGroup():
                raise MiniconfException("decode", "expected a boolean")

        async def discovery():
            await cli.discover_one(None, "app/+")

        async def invalid_json():
            await cli._handle_commands(None, ["/enabled=!"], False, 3, raw=True)

        cases = [
            (rejected, {}, ["decode", "expected a boolean"]),
            (
                invalid_json,
                {},
                ["JSON", "/enabled"],
            ),
            (discovery, {}, ["Discover", "app/+"]),
            (
                discovery,
                {"app/b": {}, "app/a": {}},
                ["Discover", "app/a\napp/b"],
            ),
        ]
        for operation, devices, expected in cases:
            with self.subTest(expected=expected):
                out, err = StringIO(), StringIO()
                with (
                    patch.object(cli, "_main", operation),
                    patch.object(
                        operations, "discover", AsyncMock(return_value=devices)
                    ),
                    redirect_stdout(out),
                    redirect_stderr(err),
                    self.assertRaises(SystemExit) as raised,
                ):
                    cli.main()
                self.assertEqual(raised.exception.code, 1)
                self.assertEqual(out.getvalue(), "")
                for context in expected:
                    self.assertIn(context, err.getvalue())
                self.assertNotIn("Traceback", err.getvalue())

    def test_discovery_success(self):
        out = StringIO()
        with (
            patch.object(
                operations, "discover", AsyncMock(return_value={"app/a": {}})
            ) as discover,
            redirect_stdout(out),
        ):
            result = asyncio.run(operations.discover_one(None, "app/+", timeout=0.5))
        self.assertEqual(result, "app/a")
        self.assertEqual(out.getvalue(), "")
        discover.assert_awaited_once_with(None, "app/+", timeout=0.5)

    def test_usage_errors_before_connect(self):
        cases = [
            ["--raw", "--prune", "", "app/id"],
            ["--raw", "app/id", "/path?"],
            *[["-t", value, "app/id", "/path"] for value in ("0", "-1", "nan", "inf")],
        ]
        for args in cases:
            with self.subTest(args=args):
                err = StringIO()
                with (
                    patch.object(sys, "argv", ["miniconf-mqtt", *args]),
                    patch.object(cli, "mqtt_client") as connect,
                    redirect_stderr(err),
                    self.assertRaises(SystemExit) as raised,
                ):
                    cli.main()
                self.assertEqual(raised.exception.code, 2)
                self.assertNotIn("Traceback", err.getvalue())
                connect.assert_not_called()

    def test_optional_resolver_import(self):
        with tempfile.TemporaryDirectory() as directory:
            package = Path(directory) / "dns"
            package.mkdir()
            (package / "__init__.py").write_text("")
            (package / "resolver.py").write_text("marker = object()\n")
            for preload in (False, True):
                with self.subTest(preload=preload):
                    result = subprocess.run(
                        [
                            sys.executable,
                            "-c",
                            """
import importlib
import sys
from miniconf_mqtt.__main__ import main

sys.path.insert(0, sys.argv[1])
preload = sys.argv[2] == "True"
resolver = importlib.import_module("dns.resolver") if preload else None
sys.argv = ["miniconf-mqtt", "--help"]
try:
    main()
except SystemExit as exc:
    assert exc.code == 0
else:
    raise AssertionError("--help did not exit")

if preload:
    assert sys.modules["dns.resolver"] is resolver
else:
    assert "dns.resolver" not in sys.modules
assert importlib.import_module("dns.resolver").marker is not None
""",
                            directory,
                            str(preload),
                        ],
                        capture_output=True,
                        text=True,
                    )
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn("usage:", result.stdout)
