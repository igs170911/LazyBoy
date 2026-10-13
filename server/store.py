"""Product records and a durable event journal; Hermes owns execution context."""
from contextlib import contextmanager
from datetime import datetime, timezone
import json
from pathlib import Path
import sqlite3
import threading
from urllib.parse import quote
import uuid


# Delegated worker statuses that mean it is still going (runtime.WORKER_LIVE).
WORKERS_RUNNING = ("queued", "running")


def now():
    return datetime.now(timezone.utc).isoformat()


def uid():
    return str(uuid.uuid4())


class Store:
    def __init__(self, directory: Path):
        directory.mkdir(parents=True, exist_ok=True)
        directory.chmod(0o700)
        self.directory = directory
        self.lock = threading.RLock()
        self.depth = 0
        self.conn = sqlite3.connect(directory / "product.sqlite3", check_same_thread=False)
        self.conn.row_factory = sqlite3.Row
        self.conn.executescript("""
        PRAGMA journal_mode=WAL;
        -- WAL keeps the database consistent with NORMAL; FULL would fsync on every streamed event.
        PRAGMA synchronous=NORMAL;
        PRAGMA busy_timeout=5000;
        CREATE TABLE IF NOT EXISTS objects(bucket TEXT,id TEXT,data TEXT NOT NULL,PRIMARY KEY(bucket,id));
        CREATE TABLE IF NOT EXISTS messages(id TEXT PRIMARY KEY,agent TEXT NOT NULL,data TEXT NOT NULL,position INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS messages_agent ON messages(agent,position);
        CREATE TABLE IF NOT EXISTS events(id INTEGER PRIMARY KEY AUTOINCREMENT,agent TEXT NOT NULL,task TEXT,kind TEXT NOT NULL,payload TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS events_agent ON events(agent,id);
        CREATE TABLE IF NOT EXISTS admissions(agent TEXT,request_id TEXT,task TEXT,payload TEXT,PRIMARY KEY(agent,request_id));
        CREATE INDEX IF NOT EXISTS messages_position ON messages(position);
        -- A bot's unfinished tasks (and finished ones whose delegated workers still run) without reading every task.
        CREATE INDEX IF NOT EXISTS tasks_open ON objects(json_extract(data,'$.agent_id'), json_extract(data,'$.state'), json_extract(data,'$.workers_live')) WHERE bucket='tasks';
        """)
        # Search reads on its own connection, so a slow LIKE never holds the lock the event loop needs.
        self.reader = sqlite3.connect(f"file:{quote(str(directory / 'product.sqlite3'))}?mode=ro", uri=True, check_same_thread=False)
        self.reader_lock = threading.Lock()

    @contextmanager
    def transaction(self):
        with self.lock:
            outer = self.depth == 0
            self.depth += 1
            try:
                yield self.conn
                if outer: self.conn.commit()
            except BaseException:
                if outer: self.conn.rollback()
                raise
            finally:
                self.depth -= 1

    def put(self, bucket, id, data):
        if bucket == "tasks" and isinstance(data.get("background"), list):
            # Kept with every write so the tasks_open index can find delegated workers that are still running.
            data = {**data, "workers_live": any(isinstance(w, dict) and w.get("status") in WORKERS_RUNNING for w in data["background"])}
        with self.transaction() as c:
            c.execute("INSERT INTO objects VALUES(?,?,?) ON CONFLICT(bucket,id) DO UPDATE SET data=excluded.data",
                      (bucket, id, json.dumps(data, ensure_ascii=False)))
        return data

    def get(self, bucket, id, default=None):
        with self.lock:
            row = self.conn.execute("SELECT data FROM objects WHERE bucket=? AND id=?", (bucket, id)).fetchone()
            return json.loads(row[0]) if row else default

    def all(self, bucket):
        with self.lock:
            return [json.loads(row[0]) for row in self.conn.execute("SELECT data FROM objects WHERE bucket=? ORDER BY rowid", (bucket,))]

    def remove(self, bucket, id):
        with self.transaction() as c:
            c.execute("DELETE FROM objects WHERE bucket=? AND id=?", (bucket, id))

    def patch(self, bucket, id, **fields):
        with self.lock:
            item = self.get(bucket, id)
            if item is None: raise KeyError(id)
            item.update(fields)
            return self.put(bucket, id, item)

    def message(self, agent, role, content, id=None, thread=None, **fields):
        id = id or uid()
        value = {"id": id, "role": role, "content": content, "at": now(), "thread": thread or agent, **fields}
        with self.transaction() as c:
            position = c.execute("SELECT COALESCE(MAX(position),0)+1 FROM messages").fetchone()[0]
            # Rewriting a message (the reply growing as it streams) moves it to the end: a user answer or steer written
            # while the reply was being written must stay above the reply that answers it.
            c.execute("INSERT INTO messages VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data, position=excluded.position",
                      (id, agent, json.dumps(value, ensure_ascii=False), position))
        return value

    @staticmethod
    def _thread_filter(agent, thread):
        # Messages written before threads existed belong to the bot's main thread, whose id is the bot id.
        if thread is None: return "", ()
        if thread == agent: return " AND COALESCE(json_extract(data,'$.thread'),agent)=agent", ()
        return " AND json_extract(data,'$.thread')=?", (thread,)

    def transcript(self, agent, thread=None):
        where, args = self._thread_filter(agent, thread)
        with self.lock:
            return [json.loads(row[0]) for row in self.conn.execute("SELECT data FROM messages WHERE agent=?" + where + " ORDER BY position", (agent, *args))]

    def get_message(self, agent, id):
        with self.lock:
            row = self.conn.execute("SELECT data FROM messages WHERE id=? AND agent=?", (id, agent)).fetchone()
            return json.loads(row[0]) if row else None

    def update_message(self, agent, id, **fields):
        """Change fields of one stored message (a reaction); its place in the conversation stays."""
        with self.transaction() as c:
            row = c.execute("SELECT data FROM messages WHERE id=? AND agent=?", (id, agent)).fetchone()
            if not row: raise KeyError(id)
            value = {**json.loads(row[0]), **fields}
            c.execute("UPDATE messages SET data=? WHERE id=? AND agent=?", (json.dumps(value, ensure_ascii=False), id, agent))
        return value

    def search_messages(self, text, limit=400):
        """Newest messages whose text contains `text` (case-insensitive for Latin letters), across bots and group chats."""
        pattern = "%" + text.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_") + "%"
        with self.reader_lock:
            rows = self.reader.execute("SELECT agent, data FROM messages WHERE json_extract(data,'$.content') LIKE ? ESCAPE '\\' ORDER BY position DESC LIMIT ?", (pattern, limit))
            return [(row[0], json.loads(row[1])) for row in rows]

    def recent_messages(self, agent, thread=None, *, before=None, limit=25):
        """Newest first: a page of one bot's (or group's) messages, older than position `before` when given."""
        where, args = self._thread_filter(agent, thread)
        if before is not None: where, args = where + " AND position<?", (*args, before)
        with self.lock:
            rows = self.conn.execute("SELECT data, position FROM messages WHERE agent=?" + where + " ORDER BY position DESC LIMIT ?", (agent, *args, limit)).fetchall()
        return [(json.loads(row[0]), row[1]) for row in rows]

    def last_message(self, agent, thread=None, keep=lambda message: True):
        """The newest message that `keep` accepts, read a page at a time instead of loading the whole conversation."""
        before = None
        while True:
            page = self.recent_messages(agent, thread, before=before)
            for message, position in page:
                if keep(message): return message
            if len(page) < 25: return None
            before = page[-1][1]

    def count_messages(self, agent, thread=None):
        where, args = self._thread_filter(agent, thread)
        with self.lock:
            return self.conn.execute("SELECT COUNT(*) FROM messages WHERE agent=?" + where, (agent, *args)).fetchone()[0]

    def clear_messages(self, agent, thread=None):
        where, args = self._thread_filter(agent, thread)
        with self.transaction() as c: c.execute("DELETE FROM messages WHERE agent=?" + where, (agent, *args))

    def event(self, agent, kind, payload, task=None):
        with self.transaction() as c:
            id = c.execute("INSERT INTO events(agent,task,kind,payload) VALUES(?,?,?,?)",
                           (agent, task, kind, json.dumps(payload, ensure_ascii=False))).lastrowid
        return {"id": id, "kind": kind, "task_id": task, "payload": payload}

    def events(self, agent, since=0, limit=300):
        with self.lock:
            rows = self.conn.execute("SELECT * FROM events WHERE agent=? AND id>? ORDER BY id LIMIT ?", (agent, since, limit))
            return [{"id": r["id"], "kind": r["kind"], "task_id": r["task"], "payload": json.loads(r["payload"])} for r in rows]

    def agent_tasks(self, agent, *, live=False):
        """One bot's tasks in the order they were created; `live` keeps only unfinished ones and those whose
        delegated workers still run (marked `workers_live`), which the tasks_open index finds without a scan."""
        sql = "SELECT data FROM objects WHERE bucket='tasks' AND json_extract(data,'$.agent_id')=?"
        if live: sql += " AND (json_extract(data,'$.state')!='terminal' OR json_extract(data,'$.workers_live')=1)"
        with self.lock:
            return [json.loads(row[0]) for row in self.conn.execute(sql + " ORDER BY rowid", (agent,))]

    def latest_task_id(self, agent):
        with self.lock:
            row = self.conn.execute("SELECT id FROM objects WHERE bucket='tasks' AND json_extract(data,'$.agent_id')=? ORDER BY rowid DESC LIMIT 1", (agent,)).fetchone()
            return row[0] if row else None

    def fields(self, bucket, id, *names):
        """A few top-level fields of one record, picked out by SQLite instead of parsing the whole JSON."""
        columns = ",".join(f"json_extract(data,'$.{name}')" for name in names)
        with self.lock:
            row = self.conn.execute(f"SELECT {columns} FROM objects WHERE bucket=? AND id=?", (bucket, id)).fetchone()
            return tuple(row) if row else None

    # Records owned by one bot: deleted with it (imports stay as provenance of created bots).
    OWNED = ("tasks", "uploads", "lessons")

    def purge_agent(self, agent):
        """Everything stored for a deleted bot outside its own data directory."""
        with self.transaction() as c:
            c.execute("DELETE FROM events WHERE agent=?", (agent,))
            c.execute("DELETE FROM admissions WHERE agent=?", (agent,))
            c.execute("DELETE FROM messages WHERE agent=?", (agent,))
            c.execute(f"DELETE FROM objects WHERE bucket IN ({','.join('?' * len(self.OWNED))}) AND json_extract(data,'$.agent_id')=?", (*self.OWNED, agent))
            c.execute("DELETE FROM objects WHERE bucket IN ('files','mcp') AND id=?", (agent,))

    def purge_orphans(self):
        """Rows left behind by bots deleted before purge_agent existed. Returns how many bots were cleaned."""
        with self.lock:
            # Group chats keep their transcript and events under the room id, like a bot does under its own.
            live = {row[0] for row in self.conn.execute("SELECT id FROM objects WHERE bucket IN ('agents','rooms')")}
            owners = {row[0] for row in self.conn.execute("SELECT agent FROM events UNION SELECT agent FROM admissions UNION SELECT agent FROM messages")}
            owners |= {row[0] for row in self.conn.execute(f"SELECT json_extract(data,'$.agent_id') FROM objects WHERE bucket IN ({','.join('?' * len(self.OWNED))})", self.OWNED) if row[0]}
            owners |= {row[0] for row in self.conn.execute("SELECT id FROM objects WHERE bucket='files'")}
        gone = owners - live
        for agent in gone: self.purge_agent(agent)
        return len(gone)

    def prune_events(self, agent, keep=5000):
        """The journal only feeds live streams and short reconnects; finished turns keep their own trail."""
        with self.transaction() as c:
            c.execute("DELETE FROM events WHERE agent=? AND id < (SELECT id FROM events WHERE agent=? ORDER BY id DESC LIMIT 1 OFFSET ?)", (agent, agent, keep - 1))

    def last_event_id(self, agent):
        with self.lock: return self.conn.execute("SELECT COALESCE(MAX(id),0) FROM events WHERE agent=?", (agent,)).fetchone()[0]

    def admission(self, agent, request_id):
        with self.lock:
            row = self.conn.execute("SELECT task,payload FROM admissions WHERE agent=? AND request_id=?", (agent, request_id)).fetchone()
            return dict(row) if row else None

    def admit(self, agent, request_id, task, payload):
        with self.transaction() as c: c.execute("INSERT INTO admissions VALUES(?,?,?,?)", (agent, request_id, task, payload))

    def agent_directory(self, id):
        # Only product-generated UUIDs are accepted as directory identities.
        if str(uuid.UUID(id)) != id: raise ValueError("Invalid bot identifier")
        root = self.directory / "bots" / id
        for name in ["hermes", "home", "workspace"]: (root / name).mkdir(parents=True, exist_ok=True)
        return root
