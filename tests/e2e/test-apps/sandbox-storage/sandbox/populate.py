"""Fills /workspace with the round-trip fixture: files of several modes and mtimes, a hard link,
symlinks (one pointing outside the tree), an SQLite database in WAL mode and a git repository."""
import os, random, sqlite3, subprocess, sys

root = "/workspace"
count = int(sys.argv[1]) if len(sys.argv) > 1 else 2000
big_mib = int(sys.argv[2]) if len(sys.argv) > 2 else 8
rng = random.Random(3035)
modes = [0o644, 0o600, 0o755, 0o640]
for i in range(count):
    d = os.path.join(root, "tree", f"d{i % 50:02d}")
    os.makedirs(d, exist_ok=True)
    p = os.path.join(d, f"f{i:05d}.txt")
    with open(p, "wb") as f:
        f.write(rng.randbytes(rng.randint(0, 4096)))
    os.chmod(p, modes[i % len(modes)])
    os.utime(p, ns=(1_700_000_000_000_000_000 + i * 1_000_000_007,) * 2)
with open(os.path.join(root, "big.bin"), "wb") as f:
    for _ in range(big_mib):
        f.write(rng.randbytes(1 << 20))
with open(os.path.join(root, "notes.txt"), "w") as f:
    f.write("persistent storage acceptance\n")
os.link(os.path.join(root, "notes.txt"), os.path.join(root, "notes-hardlink.txt"))
os.symlink("notes.txt", os.path.join(root, "notes-link"))
os.symlink("/etc/shadow", os.path.join(root, "escape"))
db = sqlite3.connect(os.path.join(root, "app.db"))
db.execute("pragma journal_mode=wal")
db.execute("create table t (id integer primary key, v text)")
db.executemany("insert into t (v) values (?)", [(f"row {i}",) for i in range(5000)])
db.commit()
db.close()
env = dict(os.environ, HOME="/home/app", GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@example.com",
           GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@example.com")
repo = os.path.join(root, "repo")
os.makedirs(repo)
subprocess.run(["git", "init", "-q", repo], check=True, env=env)
for i in range(20):
    with open(os.path.join(repo, f"src{i}.py"), "w") as f:
        f.write(f"print({i})\n")
    subprocess.run(["git", "-C", repo, "add", "-A"], check=True, env=env)
    subprocess.run(["git", "-C", repo, "commit", "-q", "-m", f"c{i}"], check=True, env=env)
os.makedirs(os.path.join(root, "scratch"), exist_ok=True)
with open(os.path.join(root, "scratch", "local-only"), "w") as f:
    f.write("excluded\n")
print("ok")
