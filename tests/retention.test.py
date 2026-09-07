"""Integration regression: run production cleanup SQL in an isolated disposable DB.
Requires the project's Postgres Compose service; never modifies the app database.
"""
from pathlib import Path
import subprocess
import uuid

ROOT = Path(__file__).resolve().parents[1]
DB = 'retention_test_' + uuid.uuid4().hex


def sql(text, database=DB):
    result = subprocess.run(['docker','compose','exec','-T','postgres','psql','-X','-q','-v','ON_ERROR_STOP=1','-U','lazyboy','-d',database], input=text, text=True, capture_output=True, cwd=ROOT)
    if result.returncode:
        raise AssertionError(result.stderr)


def check(condition):
    sql(f"DO $$ BEGIN IF NOT ({condition}) THEN RAISE EXCEPTION 'retention assertion failed'; END IF; END $$;")


def clean(name, age, batch=1000):
    query = (ROOT/'crates/api/src/retention'/f'{name}.sql').read_text()
    sql(query.replace('$1',str(age)).replace('$2',str(batch))+';')


sql(f'CREATE DATABASE {DB};','postgres')
try:
    for migration in sorted((ROOT/'migrations').glob('*.sql')):
        sql(migration.read_text())
    sql("""
    INSERT INTO users(id,name) VALUES ('u','test');
    INSERT INTO spaces(id,user_id,name) VALUES ('s','u','test');
    INSERT INTO computers(id,space_id,user_id,scope,scope_key,home_key) VALUES ('c','s','u','bot','c','c');
    INSERT INTO bots(id,space_id,user_id,name) VALUES ('b','s','u','test');
    INSERT INTO threads(id,space_id,bot_id,user_id) VALUES ('t','s','b','u');
    INSERT INTO messages(id,thread_id,seq,role,body,created_at) VALUES ('m','t',1,'user','keep my conversation',now()-interval '1 year');
    INSERT INTO events(id,thread_id,seq,type,created_at)
      SELECT 'e'||n,'t',n,'test',now()-interval '31 days' FROM generate_series(1,5) n;
    INSERT INTO events(id,thread_id,seq,type) VALUES ('recent','t',6,'test');
    INSERT INTO runs(id,space_id,bot_id,thread_id,user_id,status,checkpoint,updated_at,completed_at)
      SELECT status,'s','b','t','u',status,'{"harnessHistory":["debug"]}',now()-interval '100 days',
       CASE WHEN status IN ('completed','failed','cancelled') THEN now()-interval '100 days' ELSE NULL END
      FROM unnest(ARRAY['completed','failed','cancelled','running','queued','leased','waiting_input','waiting_takeover']) status;
    INSERT INTO runs(id,space_id,bot_id,thread_id,user_id,status,checkpoint,updated_at,completed_at)
      VALUES ('recent-completed','s','b','t','u','completed','{"keep":true}',now(),now());
    INSERT INTO run_activity(run_id,kind,payload,created_at)
      SELECT run_id,'notice','{"text":"step from an old run"}',now()-interval '10 days'
      FROM unnest(ARRAY['completed','failed','running','recent-completed']) run_id;
    INSERT INTO run_activity(run_id,kind,payload,created_at)
      VALUES ('recent-completed','notice','{"text":"fresh step"}',now());
    INSERT INTO memory_items(id,space_id,user_id,bot_id,content,revision,source_run_id)
      VALUES ('00000000-0000-0000-0000-000000000001','s','u','b','keep current memory',15,'completed');
    INSERT INTO memory_revisions(memory_id,revision,space_id,user_id,bot_id,content,importance,action)
      SELECT '00000000-0000-0000-0000-000000000001',n,'s','u','b','version',.5,'update' FROM generate_series(1,15) n;
    UPDATE memory_revisions SET created_at=now()-interval '100 days' WHERE revision IN (6,15);
    INSERT INTO memory_items(id,space_id,user_id,bot_id,content,deleted_at)
      VALUES ('00000000-0000-0000-0000-000000000002','s','u','b','deleted old',now()-interval '100 days'),
      ('00000000-0000-0000-0000-000000000003','s','u','b','deleted recent',now());
    INSERT INTO taught_skills(id,space_id,user_id,bot_id,goal,status,playbook,recording,updated_at)
      SELECT status,'s','u','b','goal',status,'{"keep":true}','{"frames":["raw"]}',now()-interval '100 days'
      FROM unnest(ARRAY['saved','failed','draft','drafting','recording']) status;
    INSERT INTO computer_profile_locks(computer_id,profile_key,bot_id,run_id,expires_at)
      VALUES ('c','old','b','completed',now()-interval '10 days'),('c','active','b','running',now()-interval '10 days');
    INSERT INTO computer_execution_leases(id,computer_id,bot_id,run_id,expires_at)
      VALUES ('old','c','b','completed',now()-interval '10 days'),('active','c','b2','running',now()-interval '10 days');
    """)
    clean('events',30,2)
    check('(SELECT count(*) FROM events)=4')
    clean('events',30)
    check("(SELECT count(*) FROM events)=1 AND EXISTS(SELECT 1 FROM events WHERE id='recent')")
    clean('checkpoints',7)
    check("(SELECT count(*) FROM runs WHERE checkpoint='{}')=3")
    clean('runs',90)
    check('(SELECT count(*) FROM runs)=6')
    check("EXISTS(SELECT 1 FROM memory_items WHERE content='keep current memory' AND source_run_id IS NULL)")
    clean('run_activity',7)
    check("(SELECT count(*) FROM run_activity)=1 AND EXISTS(SELECT 1 FROM run_activity WHERE run_id='recent-completed' AND payload->>'text'='fresh step')")
    clean('recordings',30)
    check("(SELECT count(*) FROM taught_skills WHERE recording='{}')=3 AND (SELECT count(*) FROM taught_skills WHERE playbook->>'keep'='true')=5")
    clean('revisions',90)
    check('(SELECT count(*) FROM memory_revisions)=9 AND EXISTS(SELECT 1 FROM memory_revisions WHERE revision=15)')
    clean('deleted_memories',90)
    check('(SELECT count(*) FROM memory_items)=2')
    clean('leases',7)
    clean('profile_locks',7)
    check("(SELECT count(*) FROM computer_execution_leases)=1 AND EXISTS(SELECT 1 FROM computer_execution_leases WHERE id='active')")
    check("(SELECT count(*) FROM computer_profile_locks)=1 AND EXISTS(SELECT 1 FROM computer_profile_locks WHERE profile_key='active')")
    check("EXISTS(SELECT 1 FROM messages WHERE body='keep my conversation')")
    for name,age in [('events',30),('checkpoints',7),('runs',90),('run_activity',7),('recordings',30),('revisions',90),('deleted_memories',90),('leases',7),('profile_locks',7)]:
        clean(name,age)
    check('(SELECT count(*) FROM runs)=6 AND (SELECT count(*) FROM memory_revisions)=9')
    print('PASS: all 9 retention rules; bounded batches; live work and user content preserved; repeat-safe')
finally:
    sql(f'DROP DATABASE {DB};','postgres')
