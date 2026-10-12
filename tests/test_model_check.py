import httpx
import pytest

from server.config import Settings
from server.model_check import check_model, classify, friendly, ordered
from server.store import Store


@pytest.mark.parametrize("status,text,kind", [
    (400, "Upstream request failed: Model does not support this protocol.", "protocol"),
    (400, "Upstream request failed: Model is unavailable.", "unavailable"),
    (429, "Upstream request failed: Endpoint is unavailable.", "busy"),
    (401, "Invalid API key provided", "auth"),
    (404, "Not Found", "protocol"),
    (400, "Unsupported parameter: 'max_tokens' is not supported with this model.", "ok"),
    (503, "", "busy"),
    (200, "", "ok"),
    (400, "Request is missing x-opencode-session and cannot be routed efficiently.", "unknown"),
])
def test_classify_turns_provider_errors_into_causes(status, text, kind):
    assert classify(status, text) == kind


def test_likely_protocol_is_tried_first():
    assert ordered("messages") == ["messages", "chat_completions", "responses"]
    assert ordered("bogus") == ["chat_completions", "responses", "messages"]


def transport(answers, seen):
    def handle(request):
        path = request.url.path.rsplit("/v1/", 1)[-1]
        seen.append((path, request.headers.get("x-api-key")))
        status, message = answers.get(path, (404, "Not Found"))
        return httpx.Response(status, json={"error": {"message": message}} if status >= 400 else {"ok": True})
    return httpx.MockTransport(handle)


async def test_check_finds_the_protocol_the_model_answers_on():
    seen = []
    answers = {"chat/completions": (400, "Model does not support this protocol."), "messages": (200, "")}
    result = await check_model("https://relay.example/v1", "k", "claude-haiku-5-5", "chat_completions", transport=transport(answers, seen))
    assert result["status"] == "ok" and result["protocol"] == "messages"
    assert [path for path, _ in seen] == ["chat/completions", "responses", "messages"]
    assert seen[-1][1] == "k"


async def test_check_stops_on_a_bad_key_or_busy_provider():
    seen = []
    result = await check_model("https://relay.example/v1", "k", "m", "chat_completions", transport=transport({"chat/completions": (429, "Endpoint is unavailable.")}, seen))
    assert result["status"] == "busy" and len(seen) == 1
    seen.clear()
    result = await check_model("https://relay.example/v1", "k", "m", "messages", transport=transport({"messages": (401, "Invalid API key")}, seen))
    assert result["status"] == "auth" and len(seen) == 1


async def test_a_retired_model_is_reported_unavailable_not_as_a_protocol_problem():
    seen = []
    answers = {"chat/completions": (400, "Upstream request failed: Model is unavailable.")}
    result = await check_model("https://relay.example/v1", "k", "space-bunny-free", "chat_completions", transport=transport(answers, seen))
    assert result["status"] == "unavailable" and "Model is unavailable" in result["detail"]


def test_record_check_keeps_the_working_protocol_and_saving_a_new_protocol_drops_it(tmp_path):
    settings = Settings(Store(tmp_path))
    settings.update({"provider": {"id": "relay", "base_url": "https://relay.example/v1", "api_key": "k", "models": [{"id": "claude-haiku-5-5", "protocol": "chat_completions"}]}})
    assert settings.guess_protocol("relay", "claude-haiku-5-5") == "chat_completions"
    assert settings.record_check("relay", "claude-haiku-5-5", {"status": "ok", "protocol": "messages", "message": "可以使用", "detail": "", "at": 1})
    row = settings.model_row(settings.read()["providers"]["relay"], "claude-haiku-5-5")
    assert row["protocol"] == "messages" and row["check"]["status"] == "ok"
    assert not settings.record_check("relay", "missing", {"status": "ok"})
    # Saving the same row keeps the check; a hand-picked protocol no longer vouched for drops it.
    settings.update({"provider": {"id": "relay", "models": [row]}})
    assert settings.model_row(settings.read()["providers"]["relay"], "claude-haiku-5-5")["check"]["status"] == "ok"
    settings.update({"provider": {"id": "relay", "models": [{**row, "protocol": "responses"}]}})
    assert "check" not in settings.model_row(settings.read()["providers"]["relay"], "claude-haiku-5-5")


def test_preset_model_info_guides_the_first_guess(tmp_path):
    settings = Settings(Store(tmp_path))
    assert settings.guess_protocol("opencode-zen", "claude-sonnet-5") == "messages"
    assert settings.guess_protocol("opencode-go", "gpt-5.6-luna") == "responses"


def test_failed_task_text_becomes_actionable():
    raw = "custom:product rejected this request as malformed, so the model didn't answer. Start a clean session with /new.\n\nProvider said: HTTP 400: Model does not support this protocol."
    assert "API 介面" in friendly(raw)
    assert "換一個模型" in friendly("Provider said: HTTP 400: Upstream request failed: Model is unavailable.")
    assert "稍後" in friendly("custom:product rate-limited every one of 3 attempts. Provider said: HTTP 429: Endpoint is unavailable.")
    assert friendly("工作未完成：Docker 操作逾時") is None


async def test_opencode_checks_carry_a_session_id():
    seen = []
    def handle(request):
        seen.append(request.headers.get("x-opencode-session", ""))
        return httpx.Response(200, json={"ok": True})
    result = await check_model("https://opencode.ai/zen/go/v1", "k", "deepseek-v4.1-flash", transport=httpx.MockTransport(handle))
    assert result["status"] == "ok" and seen[0].startswith("lazyboy-check-")


def test_results_from_an_outdated_checker_are_checked_again(tmp_path):
    settings = Settings(Store(tmp_path))
    settings.update({"provider": {"id": "relay", "base_url": "https://relay.example/v1", "api_key": "k", "models": [{"id": "m"}]}})
    settings.record_check("relay", "m", {"status": "unavailable", "message": "x", "detail": "", "at": 1})
    cfg = settings.read(); cfg["providers"]["relay"]["models"][0]["check"].pop("v"); settings.save(cfg)
    listed = next(p for p in settings.public()["providers"] if p["id"] == "relay")["models"][0]
    assert "check" not in listed


def test_hermes_rules_outrank_a_stored_protocol(tmp_path):
    settings = Settings(Store(tmp_path))
    settings.update({"provider": {"id": "opencode-go", "base_url": "https://opencode.ai/zen/go/v1", "api_key": "k",
                                  "models": [{"id": "deepseek-v4.1-flash", "protocol": "messages"}, {"id": "claude-haiku-5-5", "protocol": "chat_completions"}]}})
    assert settings.guess_protocol("opencode-go", "deepseek-v4.1-flash") == "chat_completions"
    assert settings.guess_protocol("custom-relay", "claude-x", {"base_url": "https://opencode.ai/zen/v1"}) == "messages"


async def test_an_unreadable_error_never_moves_a_model_to_another_protocol():
    seen = []
    answers = {"chat/completions": (400, "Something odd happened upstream"), "messages": (200, "")}
    result = await check_model("https://relay.example/v1", "k", "m", "chat_completions", transport=transport(answers, seen))
    assert result["status"] == "unknown" and len(seen) == 1


def test_a_new_key_or_url_voids_checks_and_stale_results_are_dropped(tmp_path):
    settings = Settings(Store(tmp_path))
    settings.update({"provider": {"id": "relay", "base_url": "https://relay.example/v1", "api_key": "k", "models": [{"id": "m"}]}})
    used = ("https://relay.example/v1", "k")
    assert settings.record_check("relay", "m", {"status": "ok", "protocol": "chat_completions"}, used)
    settings.update({"provider": {"id": "relay", "api_key": "k"}})  # same key again: still vouched for
    assert settings.model_row(settings.read()["providers"]["relay"], "m").get("check")
    settings.update({"provider": {"id": "relay", "api_key": "new"}})
    assert "check" not in settings.model_row(settings.read()["providers"]["relay"], "m")
    assert not settings.record_check("relay", "m", {"status": "ok", "protocol": "messages"}, used)


def test_saved_key_never_goes_to_a_supplied_url():
    from server.main import saved_key_for
    provider = {"base_url": "https://relay.example/v1", "api_key": "secret"}
    assert saved_key_for(provider, "https://relay.example/v1/") == "secret"
    assert saved_key_for(provider, "https://attacker.example/v1") == ""
