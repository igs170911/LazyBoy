import json

from server import usage
from server.store import Store


def sse(*events):
    return b"".join(b"data: " + json.dumps(e).encode() + b"\n\n" for e in events) + b"data: [DONE]\n\n"


def run(streaming, body, size=7):
    meter = usage.Meter(streaming)
    for i in range(0, len(body), size): meter.feed(body[i:i + size])
    return meter.result()


def test_chat_completions_stream_reads_the_final_usage_chunk():
    body = sse({"choices": [{"delta": {"content": "hi"}}]},
               {"choices": [], "usage": {"prompt_tokens": 1200, "completion_tokens": 40, "prompt_tokens_details": {"cached_tokens": 1000}, "cost": 0.0021}})
    assert run(True, body) == {"input": 1200, "output": 40, "cached": 1000, "reasoning": 0, "cost": 0.0021}


def test_anthropic_messages_stream_adds_cache_to_input_and_keeps_running_output():
    body = sse({"type": "message_start", "message": {"usage": {"input_tokens": 50, "cache_read_input_tokens": 900, "cache_creation_input_tokens": 10, "output_tokens": 1}}},
               {"type": "message_delta", "usage": {"output_tokens": 300}})
    result = run(True, body)
    assert result["input"] == 960 and result["cached"] == 900 and result["output"] == 300


def test_responses_stream_and_plain_json():
    body = sse({"type": "response.completed", "response": {"usage": {"input_tokens": 500, "output_tokens": 80, "input_tokens_details": {"cached_tokens": 200},
                                                                       "output_tokens_details": {"reasoning_tokens": 60}}}})
    assert run(True, body) == {"input": 500, "output": 80, "cached": 200, "reasoning": 60}
    assert run(False, json.dumps({"usage": {"prompt_tokens": 9, "completion_tokens": 3}}).encode())["input"] == 9


def test_summary_groups_by_provider_and_model(tmp_path):
    store = Store(tmp_path)
    usage.record(store, "xai", "grok-4", {"input": 100, "output": 10})
    usage.record(store, "xai", "grok-4", {"input": 50, "output": 5}, ok=False)
    usage.record(store, "xai", "grok-mini", {"input": 1, "output": 1})
    usage.record(store, "gone", "m", {"input": 7, "output": 0})
    result = usage.summary(store, 7, {"xai": {"label": "xAI"}})
    xai = result["providers"][0]
    assert (xai["label"], xai["requests"], xai["errors"], xai["input"], xai["output"]) == ("xAI", 3, 1, 151, 16)
    assert [m["model"] for m in xai["models"]] == ["grok-4", "grok-mini"]
    assert result["providers"][1]["configured"] is False


def test_a_large_responses_completion_event_still_counts():
    big = {"type": "response.completed", "response": {"instructions": "x" * 110_000, "tools": [{"name": "t"}] * 50,
                                                       "usage": {"input_tokens": 9000, "output_tokens": 700}}}
    body = sse({"type": "response.output_text.delta", "delta": "hi"}, big)
    for size in (4096, 16384, 65536):
        assert run(True, body, size)["input"] == 9000
