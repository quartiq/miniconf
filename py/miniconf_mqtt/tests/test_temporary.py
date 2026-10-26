import asyncio

import pytest

from miniconf_mqtt.temporary import TemporaryChanges


class Configuration:
    def __init__(self, values=None, fail=None, block=None):
        self.values = {"/value": 1} if values is None else dict(values)
        self.fail = fail
        self.block = block
        self.writes = []

    async def get(self, path, *, timeout):
        return self.values[path]

    async def set(self, path, value, *, timeout):
        self.writes.append((path, value))
        self.values[path] = value
        if (path, value) == self.fail:
            self.fail = None
            raise RuntimeError("write failed after application")
        if (path, value) == self.block:
            await asyncio.Future()


def test_equal_leaf_is_not_written() -> None:
    async def run():
        configuration = Configuration()
        async with TemporaryChanges(configuration) as temporary:
            await temporary.set("/value", 1)
        return configuration.writes

    assert asyncio.run(run()) == []


def test_attempted_leaves_restore_in_reverse_order() -> None:
    async def run():
        configuration = Configuration({"/a": 1, "/b": 2}, fail=("/b", 4))
        with pytest.raises(RuntimeError, match="write failed"):
            async with TemporaryChanges(configuration) as temporary:
                await temporary.update({"/a": 3, "/b": 4})
        return configuration

    configuration = asyncio.run(run())
    assert configuration.values == {"/a": 1, "/b": 2}
    assert configuration.writes == [
        ("/a", 3),
        ("/b", 4),
        ("/b", 2),
        ("/a", 1),
    ]


def test_cancelled_write_is_still_restored() -> None:
    async def run():
        configuration = Configuration({"/a": 1, "/b": 2}, block=("/b", 4))

        async def override():
            async with TemporaryChanges(configuration) as temporary:
                await temporary.update({"/a": 3, "/b": 4})

        task = asyncio.create_task(override())
        while len(configuration.writes) < 2:
            await asyncio.sleep(0)
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
        return configuration

    configuration = asyncio.run(run())
    assert configuration.values == {"/a": 1, "/b": 2}
    assert configuration.writes[-2:] == [("/b", 2), ("/a", 1)]


def test_nested_contexts_restore_the_enclosing_value() -> None:
    async def run():
        configuration = Configuration()
        async with TemporaryChanges(configuration) as temporary:
            await temporary.update({"/value": 2})
            async with TemporaryChanges(configuration) as temporary:
                await temporary.update({"/value": 3})
                assert configuration.values["/value"] == 3
            assert configuration.values["/value"] == 2
        return configuration

    configuration = asyncio.run(run())
    assert configuration.values["/value"] == 1


def test_owned_leaf_updates_do_not_repeat_reads() -> None:
    class CountingConfiguration(Configuration):
        def __init__(self):
            super().__init__()
            self.reads = []

        async def get(self, path, *, timeout):
            self.reads.append(path)
            return await super().get(path, timeout=timeout)

    async def run():
        configuration = CountingConfiguration()
        async with TemporaryChanges(configuration) as temporary:
            await temporary.set("/value", 2)
            await temporary.set("/value", 3)
        return configuration

    configuration = asyncio.run(run())
    assert configuration.reads == ["/value"]


def test_completed_scope_releases_undo_state():
    async def run():
        configuration = Configuration()
        temporary = TemporaryChanges(configuration)
        for original in (1, 3):
            configuration.values["/value"] = original
            async with temporary:
                await temporary.set("/value", 4)
            assert configuration.values["/value"] == original
            assert not temporary.owns("/value")
            assert not temporary.pending

    asyncio.run(run())


def test_registered_group_restores_in_order_and_can_be_kept():
    async def run():
        configuration = Configuration({"/group/a": 1, "/group/b": 2})
        async with TemporaryChanges(configuration) as temporary:
            temporary.register_restore("/group", (("/group/b", 2), ("/group/a", 1)))
            assert temporary.pending
            with pytest.raises(ValueError, match="overlapping"):
                temporary.register_restore("/group/a", (("/group/a", 1),))
            await temporary.update({"/group/a": 3, "/group/b": 4})
        assert configuration.writes[-2:] == [("/group/b", 2), ("/group/a", 1)]
        assert not temporary.pending
        async with temporary:
            await temporary.set("/group/a", 5)
            temporary.keep()
            assert not temporary.pending
        assert configuration.values["/group/a"] == 5

    asyncio.run(run())


@pytest.mark.parametrize("body_error", [None, ValueError("body failed")])
def test_restore_failure_preserves_body_error(body_error):
    async def run():
        configuration = Configuration({"/a": 1, "/b": 2}, fail=("/b", 2))
        with pytest.raises(BaseExceptionGroup) as caught:
            async with TemporaryChanges(configuration) as temporary:
                await temporary.update({"/a": 3, "/b": 4})
                if body_error is not None:
                    raise body_error
        assert configuration.writes[-2:] == [("/b", 2), ("/a", 1)]
        assert len(caught.value.exceptions) == 1 + (body_error is not None)
        if body_error is not None:
            assert caught.value.exceptions[0] is body_error

    asyncio.run(run())


def test_initial_noop_does_not_prevent_later_restoration():
    async def run():
        configuration = Configuration()
        with pytest.raises(RuntimeError, match="sweep failed"):
            async with TemporaryChanges(configuration) as temporary:
                await temporary.set("/value", 1)
                await temporary.set("/value", 2)
                raise RuntimeError("sweep failed")
        assert configuration.writes == [("/value", 2), ("/value", 1)]

    asyncio.run(run())
