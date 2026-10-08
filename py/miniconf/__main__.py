"""Miniconf default CLI."""

import sys


def main() -> None:
    # Paho's optional SRV resolver pulls in DNS/HTTP dependencies. The CLI uses
    # ordinary hostname resolution; suppress SRV support only during bootstrap.
    suppress_resolver = "dns.resolver" not in sys.modules
    if suppress_resolver:
        sys.modules["dns.resolver"] = None
    try:
        from .cli import main as cli_main
    finally:
        if suppress_resolver:
            del sys.modules["dns.resolver"]
    cli_main()


if __name__ == "__main__":
    main()
