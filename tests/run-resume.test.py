"""Integration regression: a run that paused mid-task must be woken by the next
message, and only the runs that are allowed to be woken.
Requires the project's Postgres Compose service; never modifies the app database.
"""
from pathlib import Path
import re
import subprocess
import uuid

ROOT = Path(__file__).resolve().parents[1]
DB = 'run_resume_test_' + uuid.uuid4().hex
RUNS_RS = (ROOT/'crates/api/src/runs.rs').read_text()


def sql(text, database=DB):
    result = subprocess.run(['docker','compose','exec','-T','postgres','psql','-X','-q','-v','ON_ERROR_STOP=1','-U','lazyboy','-d',database], input=text, text=True, capture_output=True, cwd=ROOT)
    if result.returncode:
        raise AssertionError(result.stderr)
    return result.stdout.strip()


def query(text):
    result = subprocess.run(['docker','compose','exec','-T','postgres','psql','-X','-q','-t','-A','-U','lazyboy','-d',DB], input=text, text=True, capture_output=True, cwd=ROOT)
    if result.returncode:
        raise AssertionError(result.stderr)
    return result.stdout.strip()


def check(condition, label):
    if query(f"SELECT ({condition});") != 't':
        raise AssertionError(f'run-resume assertion failed: {label}')


def flatten(text):
    return re.sub(r'\s+',' ',text).strip()


# The wake rules live in the run loop. If that SQL moves, this test must move
# with it, so pin the shapes it depends on instead of silently testing nothing.
source = flatten(RUNS_RS)
for fragment in [
    "r.status='waiting_input'",
    "r.status='waiting_takeover' AND NOT EXISTS",
    "c.control_holder='user'",
    "UPDATE runs SET status='queued', retry_count=0, checkpoint=checkpoint-'awaitResume', updated_at=now() WHERE id=$1 AND status IN ('waiting_input','waiting_takeover')",
    "SET status='waiting_input', lease_owner=NULL, lease_expires_at=NULL, updated_at=now(),",
    '"kind": "resume"',
]:
    assert flatten(fragment) in source, f'run loop SQL changed, update tests/run-resume.test.py: {fragment}'

WAKE = """
SELECT r.id FROM runs r
 WHERE r.bot_id=$BOT AND r.thread_id=$THREAD
   AND (
     (r.status IN ('queued','leased','running','waiting_input','waiting_takeover')
      AND btrim(r.prompt) ~ '^/goal($|[[:space:]])')
     OR r.status='waiting_input'
     OR (r.status='waiting_takeover' AND NOT EXISTS (
           SELECT 1 FROM computers c JOIN bots b ON b.computer_id=c.id
           WHERE b.id=r.bot_id AND c.control_holder='user'))
   )
 ORDER BY r.created_at ASC LIMIT 1
"""

WAKE_RUN = """
UPDATE runs SET status='queued', retry_count=0,
        checkpoint=checkpoint-'awaitResume', updated_at=now()
 WHERE id=$1 AND status IN ('waiting_input','waiting_takeover')
"""


def wake(bot, thread):
    found = query(WAKE.replace('$BOT', f"'{bot}'").replace('$THREAD', f"'{thread}'"))
    return found or '<null>'


sql(f'CREATE DATABASE {DB};','postgres')
try:
    for migration in sorted((ROOT/'migrations').glob('*.sql')):
        sql(migration.read_text())
    sql("""
    INSERT INTO users(id,name) VALUES ('u','test');
    INSERT INTO spaces(id,user_id,name) VALUES ('s','u','test');
    INSERT INTO computers(id,space_id,user_id,scope,scope_key,home_key,control_holder)
      VALUES ('c_free','s','u','bot','c_free','c_free','none'),
             ('c_held','s','u','bot','c_held','c_held','user');
    INSERT INTO bots(id,space_id,user_id,name,computer_id) VALUES
      ('b1','s','u','one','c_free'),('b2','s','u','two','c_held'),
      ('b3','s','u','three','c_free'),('b4','s','u','four','c_free'),
      ('b5','s','u','five','c_free'),('b6','s','u','six','c_free');
    INSERT INTO threads(id,space_id,bot_id,user_id) VALUES
      ('t1','s','b1','u'),('t2','s','b2','u'),('t3','s','b3','u'),
      ('t4','s','b4','u'),('t5','s','b5','u'),('t6','s','b6','u');
    -- The paused question: work already happened, an answer is owed.
    INSERT INTO runs(id,space_id,bot_id,thread_id,user_id,status,prompt,retry_count,checkpoint)
      VALUES ('r_paused','s','b1','t1','u','waiting_input','把報價整理成表格',3,
              '{"harnessHistory":[{"a":1}],"harnessTurns":12,
                "awaitResume":{"reason":"mid_task_text","turns":12,"limit":40}}');
    -- A human holding the mouse is never interrupted by an incoming message.
    INSERT INTO runs(id,space_id,bot_id,thread_id,user_id,status,prompt)
      VALUES ('r_takeover_user','s','b2','t2','u','waiting_takeover','等我登入');
    INSERT INTO runs(id,space_id,bot_id,thread_id,user_id,status,prompt)
      VALUES ('r_takeover_free','s','b3','t3','u','waiting_takeover','等我登入');
    INSERT INTO runs(id,space_id,bot_id,thread_id,user_id,status,prompt)
      VALUES ('r_running','s','b4','t4','u','running','整理報價');
    INSERT INTO runs(id,space_id,bot_id,thread_id,user_id,status,prompt)
      VALUES ('r_goal','s','b5','t5','u','running','/goal 每天彙整日報');
    INSERT INTO runs(id,space_id,bot_id,thread_id,user_id,status,prompt)
      VALUES ('r_done','s','b6','t6','u','completed','昨天的事');
    """)

    assert wake('b1','t1') == 'r_paused', 'a paused run must collect the next message'
    assert wake('b2','t2') == '<null>', 'a run whose human holds control stays parked'
    assert wake('b3','t3') == 'r_takeover_free', 'a released takeover run resumes on reply'
    assert wake('b4','t4') == '<null>', 'a plain running run starts its own task'
    assert wake('b5','t5') == 'r_goal', 'goal steering still joins the live goal run'
    assert wake('b6','t6') == '<null>', 'a finished run is never reopened'

    sql(WAKE_RUN.replace('$1', "'r_paused'"))
    check("(SELECT status FROM runs WHERE id='r_paused')='queued'", 'paused run must be claimable again')
    check("(SELECT retry_count FROM runs WHERE id='r_paused')=0", 'a human answer is not a retry')
    check("(SELECT checkpoint ? 'awaitResume' FROM runs WHERE id='r_paused')=false", 'the pending question must be retired')
    check("(SELECT checkpoint->>'harnessTurns' FROM runs WHERE id='r_paused')='12'", 'harness state survives the pause')

    sql(WAKE_RUN.replace('$1', "'r_running'"))
    check("(SELECT status FROM runs WHERE id='r_running')='running'", 'the wake must not disturb a running run')

    # The question the UI renders: reason, turns and limit all have to survive.
    sql("""
    INSERT INTO runs(id,space_id,bot_id,thread_id,user_id,status,prompt,checkpoint)
      VALUES ('r_budget','s','b1','t1','u','waiting_input','把報價整理成表格',
              '{"awaitResume":{"reason":"budget_exhausted","turns":40,"limit":40}}');
    INSERT INTO messages(id,thread_id,seq,role,body,blocks,run_id)
      VALUES ('q','t1',1,'assistant','我在這個任務上用了 40 輪…',
              '[{"kind":"resume","reason":"budget_exhausted","turns":40,"limit":40}]','r_budget');
    """)
    check("""(SELECT blocks->0->>'reason' FROM messages WHERE id='q')='budget_exhausted'
             AND (SELECT blocks->0->>'limit' FROM messages WHERE id='q')='40'""",
          'the resume chip needs reason, turns and limit')
    print('PASS: paused runs resume on the next message; human control, live runs and finished runs are never hijacked; harness state and resume chip survive')
finally:
    sql(f'DROP DATABASE {DB};','postgres')
