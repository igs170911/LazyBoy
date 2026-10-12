from server.computers import seed_skills
from server.files import important, rank, walk


def test_results_are_important_and_build_noise_is_not():
    for rel in ["report.md", "projects/site/index.html", "charts/sales.png", "projects/app/src/main.py"]:
        assert important(rel), rel
    for rel in ["projects/app/dist/bundle.js", "package-lock.json", "run.log", "projects/app/cache/x.json", "a/b.pyc"]:
        assert not important(rel), rel


def test_deliverables_rank_before_deep_source():
    assert rank("report.md") < rank("projects/app/src/main.py")
    assert rank("report.md") < rank("notes/report.md")


def test_walk_skips_dependency_trees_and_hidden(tmp_path):
    (tmp_path / "node_modules/pkg").mkdir(parents=True)
    (tmp_path / "node_modules/pkg/index.js").write_text("x")
    (tmp_path / ".git").mkdir()
    (tmp_path / ".git/HEAD").write_text("x")
    (tmp_path / "out.md").write_text("x")
    assert [p.name for p in walk(tmp_path)] == ["out.md"]


def test_builtin_skills_seed_once_and_respect_deletion(tmp_path):
    seed_skills(tmp_path)
    skill = tmp_path / "grok-build" / "SKILL.md"
    assert "grok --no-auto-update" in skill.read_text()
    skill.unlink(); skill.parent.rmdir()
    seed_skills(tmp_path)
    assert not skill.exists()
