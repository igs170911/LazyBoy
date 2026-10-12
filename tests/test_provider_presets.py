from urllib.parse import urlparse

from server.config import PROVIDER_PRESETS


def test_presets_come_from_hermes_and_fit_the_proxy():
    ids = [p["id"] for p in PROVIDER_PRESETS]
    assert len(ids) == len(set(ids)) and len(ids) > 20
    assert {"openrouter", "anthropic", "openai-api", "deepseek", "gemini", "opencode-go"} <= set(ids)
    assert not {"custom", "copilot", "openai-codex", "nous", "bedrock"} & set(ids)
    for preset in PROVIDER_PRESETS:
        assert preset["label"] and preset["protocol"] in {"chat_completions", "responses", "messages"}
        if preset["base_url"]:
            assert urlparse(preset["base_url"]).scheme in {"http", "https"} and not preset["base_url"].endswith("/")
        if preset["protocol"] == "messages":
            assert preset["base_url"].endswith("/v1")


def test_known_presets_match_native_endpoints():
    by_id = {p["id"]: p for p in PROVIDER_PRESETS}
    assert by_id["anthropic"]["base_url"] == "https://api.anthropic.com/v1"
    assert by_id["openai-api"]["protocol"] == "responses"
    assert by_id["gemini"]["base_url"].endswith("/openai")
    assert by_id["lmstudio"]["local"] is True


def test_presets_carry_per_model_protocol_and_capabilities():
    by_id = {p["id"]: p for p in PROVIDER_PRESETS}
    for preset in PROVIDER_PRESETS:
        assert set(preset["model_info"]) == set(preset["models"])
        for info in preset["model_info"].values():
            assert info["protocol"] in {"chat_completions", "responses", "messages"} and isinstance(info["vision"], bool)
            assert info["efforts"] is None or isinstance(info["efforts"], list)
    zen = by_id["opencode-zen"]["model_info"]
    assert all(v["protocol"] == "messages" for k, v in zen.items() if k.startswith("claude-"))
    assert all(v["protocol"] == "responses" for k, v in zen.items() if k.startswith("gpt-"))
