"""The image command. While /workspace/kill/on exists it commits a row to an SQLite database in WAL
mode every 50 ms and rewrites a file through rename, so an abrupt termination lands mid-write."""
import os, sqlite3, time

kill = "/workspace/kill"
db = None
while True:
    if not os.path.exists(os.path.join(kill, "on")):
        if db is not None:
            db.close()
            db = None
        time.sleep(0.2)
        continue
    if db is None:
        db = sqlite3.connect(os.path.join(kill, "w.db"))
        db.execute("pragma journal_mode=wal")
        db.execute("create table if not exists t (id integer primary key, ts real, pad blob)")
        db.commit()
    now = time.time()
    db.execute("insert into t (ts, pad) values (?, randomblob(512))", (now,))
    db.commit()
    tmp = os.path.join(kill, "renamed.tmp")
    with open(tmp, "w") as f:
        f.write(f"{now}\n")
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp, os.path.join(kill, "renamed"))
    time.sleep(0.05)
