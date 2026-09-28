"""CLI bootstrap avoids optional SRV imports without masking other callers."""

import subprocess
import sys
import tempfile
from pathlib import Path
from unittest import TestCase


class CliTests(TestCase):
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
from miniconf.__main__ import main

sys.path.insert(0, sys.argv[1])
preload = sys.argv[2] == "True"
resolver = importlib.import_module("dns.resolver") if preload else None
sys.argv = ["miniconf", "--help"]
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
