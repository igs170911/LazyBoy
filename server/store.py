"""Product records and a durable event journal; Hermes owns execution context."""
from contextlib import contextmanager
from datetime import datetime, timezone
import json
from pathlib import Path
import sqlite3
import threading
import uuid


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
        PRAGMA busy_timeout=5000;
        CREATE TABLE IF NOT EXISTS objects(bucket TEXT,id TEXT,data TEXT NOT NULL,PRIMARY KEY(bucket,id));
        CREATE TABLE IF NOT EXISTS messages(id TEXT PRIMARY KEY,agent TEXT NOT NULL,data TEXT NOT NULL,position INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS messages_agent ON messages(agent,position);
        CREATE TABLE IF NOT EXISTS events(id INTEGER PRIMARY KEY AUTOINCREMENT,agent TEXT NOT NULL,task TEXT,kind TEXT NOT NULL,payload TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS events_agent ON events(agent,id);
        CREATE TABLE IF NOT EXISTS admissions(agent TEXT,request_id TEXT,task TEXT,payload TEXT,PRIMARY KEY(agent,request_id));
        """)

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
            c.execute("INSERT INTO messages VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
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
            live = {row[0] for row in self.conn.execute("SELECT id FROM objects WHERE bucket='agents'")}
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
