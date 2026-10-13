import pytest

from server import main


@pytest.mark.asyncio
async def test_model_requests_share_one_pooled_client():
    first = main.model_http()
    assert main.model_http() is first and not first.is_closed
    await main.close_model_http()
    assert first.is_closed
    second = main.model_http()
    assert second is not first and not second.is_closed
    await main.close_model_http()


def test_shutdown_signal_handling_is_reentrant(monkeypatch):
    """A second stop signal can arrive while the first is still being handled (Ctrl+C reaches uv and Python)."""
    import asyncio
    monkeypatch.setattr(main, "stopping", False)
    woken = []
    class Loop:
        def call_soon_threadsafe(self, callback):
            main.begin_shutdown(self)  # the second signal, landing inside the first handler
            woken.append(callback)
    wake = asyncio.Event()
    monkeypatch.setitem(main.runtime.listeners, "bot", {wake})
    main.begin_shutdown(Loop())
    assert main.stopping is True and len(woken) == 1
    woken[0]()
    assert wake.is_set()
