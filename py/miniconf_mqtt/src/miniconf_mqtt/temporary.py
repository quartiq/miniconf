"""Temporary leaf ownership and reverse-order restoration."""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass, field
from typing import Protocol


class _ReadWrite(Protocol):
    async def get(self, path: str, *, timeout: float) -> object: ...

    async def set(self, path: str, value: object, *, timeout: float) -> None: ...


@dataclass
class TemporaryChanges:
    """Scoped undo log: record before each first write, restore on exit.

    All mutations must use this object. Concurrent writers and recovery from
    process/device failure are unsupported.
    Restoration attempts every registered write and groups failures with any
    exception from the body.
    """

    interface: _ReadWrite
    timeout: float = 3.0
    _restores: dict[str, tuple[tuple[str, object], ...]] = field(
        default_factory=dict, init=False
    )

    @property
    def pending(self) -> bool:
        """Whether any restoration remains registered."""
        return bool(self._restores)

    def owns(self, path: str) -> bool:
        """Whether a leaf or enclosing subtree already has a registered restore."""
        return any(
            path == root or path.startswith(f"{root}/") for root in self._restores
        )

    def register_restore(
        self, root: str, writes: tuple[tuple[str, object], ...]
    ) -> None:
        """Claim a nonoverlapping subtree before mutation; replay writes in order.

        Application-specific selectors can register their restoration here
        without teaching the generic leaf setter about application schemas.
        """
        if any(
            root == owned
            or root.startswith(f"{owned}/")
            or owned.startswith(f"{root}/")
            for owned in self._restores
        ):
            raise ValueError(f"overlapping temporary changes: {root}")
        self._restores[root] = writes

    async def set(
        self, path: str, value: object, *, timeout: float | None = None
    ) -> None:
        """Set one leaf, recording its original before the first attempted change."""

        timeout = self.timeout if timeout is None else timeout
        if not self.owns(path):
            current = await self.interface.get(path, timeout=timeout)
            if current == value:
                return
            self.register_restore(path, ((path, current),))
        await self.interface.set(path, value, timeout=timeout)

    async def update(self, values: Mapping[str, object]) -> None:
        """Apply independent leaves in mapping order."""

        for path, value in values.items():
            await self.set(path, value)

    def keep(self) -> None:
        """Discard the registered restorations."""
        self._restores.clear()

    async def restore(self) -> list[tuple[str, BaseException]]:
        """Restore owned mutations in reverse action order."""

        errors: list[tuple[str, BaseException]] = []
        while self._restores:
            label, writes = self._restores.popitem()
            for path, value in writes:
                try:
                    await self.interface.set(path, value, timeout=self.timeout)
                except BaseException as error:
                    errors.append((label, error))
        return errors

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, cause, traceback):
        errors = await self.restore()
        if not errors:
            return
        failures = [cause] if cause is not None else []
        for path, error in errors:
            error.add_note(f"restoring {path}")
            failures.append(error)
        raise BaseExceptionGroup(
            "temporary changes restoration failed", failures
        ) from None
