"""Miniconf default CLI (async)"""

import asyncio
from .async_ import _main

if __name__ == "__main__":
    asyncio.run(_main())
