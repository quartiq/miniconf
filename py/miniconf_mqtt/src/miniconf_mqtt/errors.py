"""Miniconf operation and schema errors."""


class MiniconfException(Exception):
    """A Miniconf error with a machine-readable code and explanation."""

    def __init__(self, code, message):
        self.code = code
        self.message = message
        super().__init__(code, message)

    def __repr__(self):
        return f"{self.code}: {self.message}"
