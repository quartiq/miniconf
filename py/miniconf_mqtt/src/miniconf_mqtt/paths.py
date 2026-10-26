"""Miniconf slash-separated paths."""

from .errors import MiniconfException


def validate_path(path: str) -> str:
    """Validate one Miniconf slash-separated path."""
    if path and path[0] != "/":
        raise MiniconfException("Path", "Path must be empty or start with '/'")
    return path


def subtree_match(path: str, root: str) -> bool:
    """Whether `path` is equal to or below `root`."""
    root = validate_path(root)
    return not root or path == root or path.startswith(f"{root}/")
