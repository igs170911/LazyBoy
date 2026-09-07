"""Integration regression: the run trail the hover bubble reads, and the manual
retry that re-queues a dead run without losing where it stopped.
Requires the project's Postgres Compose service; never modifies the app database.
"""
from pathlib import Path
import re
import subprocess
import uuid

ROOT = Path(__file__).resolve().parents[1]
DB = 'run_activity_test_' + uuid.uuid4().hex
MONITOR_RS = (ROOT/'crates/api/src/monitor.rs').read_text()
RUNS_RS = (ROOT/'crates/api/src/runs.rs').read_text()
ACTIVITY_SQL = (ROOT/'crates/api/src/retention/run_activity.sql').read_text()


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
        raise AssertionError(f'run-activity assertion failed: {label}')


def flatten(text):
    return re.sub(r'\s+',' ',text).strip()


def trail(run, after=None, limit=60):
    """The handler's paging query: newest window first read, then forward by id."""
    cursor = 'NULL' if after is None else str(after)
    statement = """SELECT id FROM ( SELECT id FROM run_activity WHERE run_id='%s' AND (%s::bigint IS NULL OR id>%s) ORDER BY id DESC LIMIT %s ) recent ORDER BY id ASC""" % (run, cursor, cursor, limit)
    return query(statement)


def row_ids(run):
    return query(f"SELECT id FROM run_activity WHERE run_id='{run}' ORDER BY id;").split()


def retry(run):
    """The handler's retry UPDATE, verbatim apart from the bind parameters."""
    statement = """
    UPDATE runs SET status='queued', retry_count=0, error=NULL, completed_at=NULL,
            lease_owner=NULL, lease_expires_at=NULL, updated_at=now()
     WHERE id='%s' AND space_id='s' AND user_id='u'
       AND status IN ('failed','cancelled')
       AND NOT EXISTS (
           SELECT 1 FROM runs a
           WHERE a.bot_id=runs.bot_id AND a.id<>runs.id
             AND a.status IN ('leased','running','waiting_input','waiting_takeover')
             AND (a.lease_expires_at IS NULL OR a.lease_expires_at >= now())
       )
     RETURNING thread_id""" % run
    return query(statement) or '<null>'


# The bubble is only as honest as what the run loop writes. Pin the shapes so a
# silent rename shows up here instead of as a blank panel.
monitor = flatten(MONITOR_RS)
for fragment in [
    '.route("/api/runs/{id}/activity", get(activity))',
    '.route("/api/runs/{id}/retry", post(retry))',
    'INSERT INTO run_activity (run_id,kind,payload)',
    'SELECT id, kind, payload, created_at FROM run_activity WHERE run_id=$1 AND ($2::bigint IS NULL OR id>$2) ORDER BY id DESC LIMIT $3',
    "(checkpoint->>'turn')::bigint AS turn",
    "(checkpoint->>'turnLimit')::bigint AS turn_limit",
    "AND status IN ('failed','cancelled')",
    'a.bot_id=runs.bot_id AND a.id<>runs.id',
    "a.status IN ('leased','running','waiting_input','waiting_takeover')",
    '"run is not retryable"',
]:
    assert flatten(fragment) in monitor, f'monitor SQL changed, update tests/run-activity.test.py: {fragment}'

runs_source = flatten(RUNS_RS)
for fragment in [
    '"kind": "error"',
    '"code": failure.code',
    '"Worker interrupted after tool execution',
    "'step', $2::text, 'stepAt', now(), 'turn', $3::bigint, 'turnLimit', $4::bigint",
    '"event": "started"',
    '"event": "failed"',
    '"status": tool_status(',
    '"elapsedMs": model_elapsed',
]:
    assert flatten(fragment) in runs_source, f'run loop instrumentation changed, update tests/run-activity.test.py: {fragment}'

# template0, not template1: hosts whose Postgres collation version drifted refuse to
# copy template1, and C collation keeps this test's ordering deterministic anyway.
sql(f"CREATE DATABASE {DB} TEMPLATE template0 LC_COLLATE 'C' LC_CTYPE 'C';",'postgres')
try:
    for migration in sorted((ROOT/'migrations').glob('*.sql')):
        sql(migration.read_text())

    # 015 has to survive being applied twice, like every other migration.
    sql((ROOT/'migrations/015_run_activity.sql').read_text())
    check("""(SELECT count(*) FROM pg_indexes WHERE tablename='run_activity' AND indexname IN ('run_activity_run_idx','run_activity_retention_idx'))=2""",
          'the trail needs its run index and its retention index')

    sql("""
    INSERT INTO users(id,name) VALUES ('u','test');
    INSERT INTO spaces(id,user_id,name) VALUES ('s','u','test');
    INSERT INTO computers(id,space_id,user_id,scope,scope_key,home_key) VALUES ('c','s','u','bot','c','c');
    INSERT INTO bots(id,space_id,user_id,name,computer_id) VALUES
      ('b_live','s','u','live','c'),('b_idle','s','u','idle','c'),
      ('b_busy','s','u','busy','c'),('b_other','s','u','other','c');
    INSERT INTO threads(id,space_id,bot_id,user_id) VALUES
      ('t_live','s','b_live','u'),('t_idle','s','b_idle','u'),
      ('t_busy','s','b_busy','u'),('t_other','s','b_other','u');
    INSERT INTO runs(id,space_id,bot_id,thread_id,user_id,status,prompt,checkpoint) VALUES
      ('r_live','s','b_live','t_live','u','running','整理報價','{"step":"browser: click #12","turn":3,"turnLimit":40}'),
      ('r_goal','s','b_live','t_live','u','running','/goal 每天彙整日報','{"step":"shell: npm test","turn":7,"turnLimit":null}'),
      ('r_failed','s','b_idle','t_idle','u','failed','整理報價','{"step":"browser: click #12","turn":7}'),
      ('r_cancelled','s','b_idle','t_idle','u','cancelled','整理報價','{"keep":true}'),
      ('r_busy_running','s','b_busy','t_busy','u','running','還沒結束','{}'),
      ('r_busy_failed','s','b_busy','t_busy','u','failed','整理報價','{}'),
      ('r_other','s','b_other','t_other','u','running','別人的工作正在跑','{}');
    UPDATE runs SET error='HTTP status 401 Unauthorized: invalid api key' WHERE id='r_failed';
    UPDATE runs SET retry_count=3, lease_owner='stale-worker', lease_expires_at=now()+interval '5 minutes' WHERE id='r_failed';
    """)

    # The header the bubble reads: turn, limit and step must come back typed.
    check("""(SELECT (checkpoint->>'turn')::bigint FROM runs WHERE id='r_live')=3
             AND (SELECT (checkpoint->>'turnLimit')::bigint FROM runs WHERE id='r_live')=40""",
          'turn and turn limit must be readable as numbers')
    check("""(SELECT (checkpoint->>'turnLimit')::bigint IS NULL FROM runs WHERE id='r_goal')""",
          'a goal run has no limit instead of a broken one')

    # A trail, oldest first, with the payload the panel renders.
    sql("""
    INSERT INTO run_activity(run_id,kind,payload) VALUES
      ('r_live','run','{"event":"started","task":"整理報價"}'),
      ('r_live','model','{"turn":1,"elapsedMs":6400,"toolCalls":1,"text":"先打開網頁"}'),
      ('r_live','tool','{"turn":1,"name":"browser","step":"browser: click #12","status":"ok","elapsedMs":400}'),
      ('r_live','retry','{"turn":2,"attempt":1,"error":"429 rate limit","gaveUp":false}'),
      ('r_live','tool','{"turn":2,"name":"browser","step":"browser: type #13","status":"timed_out","elapsedMs":150000}');
    INSERT INTO run_activity(run_id,kind,payload) VALUES ('r_failed','run','{"event":"failed","error":"HTTP status 401"}');
    """)
    assert len(row_ids('r_live')) == 5, 'the trail collected every turn'
    check("""(SELECT payload->>'step' FROM run_activity WHERE kind='tool' ORDER BY id LIMIT 1)='browser: click #12'""",
          'a tool line carries the step it is on')

    # Paging: the first read is the newest window (oldest first so it renders in
    # order), later reads only carry what is newer than the row already shown.
    live = row_ids('r_live')
    assert len(live) == 5, f'expected five trail rows, got {live}'
    assert trail('r_live', None, 60).split() == live, 'a full window is the whole trail, oldest first'
    assert trail('r_live', None, 2).split() == live[3:], f'the first read must be the newest window, got {trail("r_live", None, 2).split()}'
    assert trail('r_live', live[2], 60).split() == live[3:], 'a cursor must never serve a row twice'
    assert trail('r_live', live[4], 60) == '', 'a tail read on a quiet run stays empty'
    assert trail('r_nothing', None, 60) == '', 'a run with no trail shows an empty panel, not an error'
    sql("""
    INSERT INTO run_activity(run_id,kind,payload) VALUES
      ('r_live','tool','{"turn":3,"name":"shell","step":"shell: npm run build","status":"ok"}'),
      ('r_live','notice','{"turn":3,"text":"這輪不需要電腦"}');
    """)
    fresh = row_ids('r_live')[5:]
    assert len(fresh) == 2, f'two new rows expected, got {fresh}'
    assert trail('r_live', live[4], 60).split() == fresh, 'new rows extend the panel in arrival order'
    assert trail('r_live', live[4], 1) == fresh[1], 'a small window keeps the freshest line, not the oldest'
    assert len(set(trail('r_live', None, 60).split())) == 7, 'the window never repeats a row'

    # Manual retry: back to queued, counters reset, checkpoint kept for the resume.
    assert retry('r_failed') == 't_idle', 'a failed run must be re-queueable from the chat'
    check("""(SELECT status FROM runs WHERE id='r_failed')='queued'""", 'a retry goes back to the queue')
    check("""(SELECT retry_count FROM runs WHERE id='r_failed')=0""", 'a human retry is not the automatic retry')
    check("""(SELECT error FROM runs WHERE id='r_failed') IS NULL""", 'the stale failure must stop being shown')
    check("""(SELECT lease_owner FROM runs WHERE id='r_failed') IS NULL""", 'a re-queued run must not keep the dead worker lease')
    check("""(SELECT checkpoint->>'step' FROM runs WHERE id='r_failed')='browser: click #12'""",
          'the checkpoint survives so the run continues where it stopped')
    assert retry('r_failed') == '<null>', 'a queued run cannot be re-queued twice'
    assert retry('r_cancelled') == 't_idle', 'a cancelled run can be restarted too'

    # Only the bot that is actually idle may be restarted, and only for itself.
    assert retry('r_busy_running') == '<null>', 'a live run is never hijacked by a retry'
    assert retry('r_busy_failed') == '<null>', 'a bot with work in flight cannot be double-booked'
    sql("UPDATE runs SET status='cancelled' WHERE id='r_busy_running'")
    assert retry('r_busy_failed') == 't_busy', 'once its other run is gone the retry works'
    assert retry('r_other') == '<null>', 'another bot is none of this retry\'s business'

    # Diagnostics are expendable: old trail lines expire, fresh ones stay.
    sql("""
    INSERT INTO run_activity(run_id,kind,payload,created_at)
      SELECT 'r_live','notice','{"text":"old"}',now()-interval '30 days' FROM generate_series(1,3) n;
    UPDATE run_activity SET created_at=now()-interval '30 days' WHERE payload->>'text'='old';
    """)
    stale = query("SELECT count(*) FROM run_activity WHERE created_at < now()-interval '7 days';")
    sql(ACTIVITY_SQL.replace('$1','7').replace('$2','1000')+';')
    removed = int(stale) - int(query("SELECT count(*) FROM run_activity WHERE created_at < now()-interval '7 days';"))
    assert removed == int(stale), f'retention must remove every expired row ({stale}), removed {removed}'
    check("""(SELECT count(*) FROM run_activity WHERE run_id='r_live')=7""", 'fresh trail rows are never collected')
    sql(ACTIVITY_SQL.replace('$1','7').replace('$2','1000')+';')
    check("""(SELECT count(*) FROM run_activity WHERE run_id='r_live')=7""", 'repeat retention batches stay safe')

    # And the trail dies with its run, never as an orphan holding the table open.
    sql("DELETE FROM runs WHERE id='r_live';")
    check("""(SELECT count(*) FROM run_activity WHERE run_id='r_live')=0""", 'deleting a run must delete its trail')
    print('PASS: trail paging, typed turn/limit header, guarded manual retry that keeps the checkpoint, expiring diagnostics, cascading cleanup')
finally:
    sql(f'DROP DATABASE {DB};','postgres')
