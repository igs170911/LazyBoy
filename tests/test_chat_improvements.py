"""Test that the improved chat experience features are working as expected."""
import sys
import os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

from server.computers import Computers
from server.config import Settings
from server.store import Store, uid


def test_soul_prompt_improvements(tmp_path):
    """Verify that SOUL.md contains key improvements for better chat experience."""
    store = Store(tmp_path)
    settings = Settings(store)
    computers = Computers(store, settings)
    
    # Create a test agent
    agent_id = uid()
    agent = {
        "id": agent_id,
        "name": "測試Bot",
        "title": "測試用助理",
        "persona": "幫助使用者完成各種任務",
        "created_at": "2024-01-01T00:00:00Z",
        "hermes_session": None,
    }
    store.put("agents", agent_id, agent)
    
    # Prepare the agent (generates SOUL.md)
    computers.prepare(agent_id)
    
    # Read the generated SOUL.md
    soul_path = store.agent_directory(agent_id) / "hermes" / "SOUL.md"
    soul_content = soul_path.read_text()
    
    # Check for instant acknowledgement requirement
    assert "兩秒內必須回應" in soul_content, "SOUL should require 2-second response time"
    assert "先用一句話確認收到" in soul_content, "SOUL should mention acknowledging before work"
    
    # Check for concise reply style
    assert "簡潔、直接、溫暖" in soul_content, "SOUL should specify concise style"
    assert "先給答案或結果" in soul_content, "SOUL should emphasize leading with the answer"
    
    # Check for milestone-only progress
    assert "重要里程碑" in soul_content, "SOUL should mention milestone-only updates"
    assert "命令級的旁白" not in soul_content or "不要命令級的旁白" in soul_content, "SOUL should avoid tool-level narration"
    
    # Check for proactive completion
    assert "完成後主動回報" in soul_content, "SOUL should require proactive completion reporting"
    
    # Check for error recovery guidance
    assert "換方法再試" in soul_content, "SOUL should mention trying alternative methods"
    assert "不輕易放棄" in soul_content or "不要直接放棄" in soul_content, "SOUL should encourage persistence"
    
    # Check that friendly tone is encouraged
    assert "您好" in soul_content and "不要用公務口吻" in soul_content, "SOUL should discourage formal language"
    
    print("✓ SOUL.md contains all key chat experience improvements")


def test_soul_prompt_with_primary_bot(tmp_path):
    """Verify that primary bot gets the coordination section."""
    store = Store(tmp_path)
    settings = Settings(store)
    computers = Computers(store, settings)
    
    # Create a primary agent
    agent_id = uid()
    agent = {
        "id": agent_id,
        "name": "主要Bot",
        "title": "主要助理",
        "persona": "協調其他 Bot",
        "created_at": "2024-01-01T00:00:00Z",
        "hermes_session": None,
        "primary": True,  # Mark as primary
    }
    store.put("agents", agent_id, agent)
    
    # Prepare the agent
    computers.prepare(agent_id)
    
    # Read the generated SOUL.md
    soul_path = store.agent_directory(agent_id) / "hermes" / "SOUL.md"
    soul_content = soul_path.read_text()
    
    # Check for primary bot section
    assert "主要 Bot" in soul_content, "Primary bot should have coordination section"
    assert "message_bot" in soul_content, "Primary bot should know about message_bot tool"
    
    print("✓ Primary bot SOUL.md contains coordination instructions")


def test_soul_prompt_with_reply_styles(tmp_path):
    """Verify that different reply styles are applied correctly."""
    store = Store(tmp_path)
    settings = Settings(store)
    computers = Computers(store, settings)
    
    # Test brief style
    brief_agent_id = uid()
    brief_agent = {
        "id": brief_agent_id,
        "name": "簡短Bot",
        "created_at": "2024-01-01T00:00:00Z",
        "hermes_session": None,
        "reply_style": "brief",
    }
    store.put("agents", brief_agent_id, brief_agent)
    computers.prepare(brief_agent_id)
    
    brief_soul = (store.agent_directory(brief_agent_id) / "hermes" / "SOUL.md").read_text()
    assert "極度精簡" in brief_soul, "Brief style should emphasize conciseness"
    
    # Test detailed style
    detailed_agent_id = uid()
    detailed_agent = {
        "id": detailed_agent_id,
        "name": "詳細Bot",
        "created_at": "2024-01-01T00:00:00Z",
        "hermes_session": None,
        "reply_style": "detailed",
    }
    store.put("agents", detailed_agent_id, detailed_agent)
    computers.prepare(detailed_agent_id)
    
    detailed_soul = (store.agent_directory(detailed_agent_id) / "hermes" / "SOUL.md").read_text()
    assert "完整說明" in detailed_soul, "Detailed style should allow full explanations"
    
    print("✓ Reply styles are correctly applied to SOUL.md")


def test_soul_prompt_with_ask_first_items(tmp_path):
    """Verify that ask_first items are included in SOUL.md."""
    store = Store(tmp_path)
    settings = Settings(store)
    computers = Computers(store, settings)
    
    agent_id = uid()
    agent = {
        "id": agent_id,
        "name": "謹慎Bot",
        "created_at": "2024-01-01T00:00:00Z",
        "hermes_session": None,
        "ask_first": ["send", "publish", "pay", "delete"],
    }
    store.put("agents", agent_id, agent)
    computers.prepare(agent_id)
    
    soul_content = (store.agent_directory(agent_id) / "hermes" / "SOUL.md").read_text()
    
    # Check that ask_first items are mentioned
    assert "寄出或傳送訊息" in soul_content, "Should mention 'send' action"
    assert "公開發佈" in soul_content, "Should mention 'publish' action"
    assert "付款" in soul_content, "Should mention 'pay' action"
    assert "刪除" in soul_content, "Should mention 'delete' action"
    assert "先取得使用者同意" in soul_content, "Should mention getting user approval"
    
    print("✓ Ask-first items are correctly included in SOUL.md")


if __name__ == "__main__":
    import tempfile
    import shutil
    
    # Run tests
    with tempfile.TemporaryDirectory() as tmpdir:
        from pathlib import Path
        tmp = Path(tmpdir)
        
        test_soul_prompt_improvements(tmp / "test1")
        test_soul_prompt_with_primary_bot(tmp / "test2")
        test_soul_prompt_with_reply_styles(tmp / "test3")
        test_soul_prompt_with_ask_first_items(tmp / "test4")
    
    print("\n✅ All chat experience improvement tests passed!")
