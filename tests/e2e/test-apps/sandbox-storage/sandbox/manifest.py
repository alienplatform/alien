"""Prints a JSON manifest of /workspace (minus the kill-run directory): type, mode, owner, mtime,
size, content hash, link target and hard-link group of every entry, plus SQLite and git checks."""
import hashlib, json, os, sqlite3, stat, subprocess

root = "/workspace"
skip = {"kill"}
entries, inodes = {}, {}
for dirpath, dirnames, filenames in os.walk(root):
    rel_dir = os.path.relpath(dirpath, root)
    if rel_dir == ".":
        dirnames[:] = [d for d in dirnames if d not in skip]
    for name in dirnames + filenames:
        p = os.path.join(dirpath, name)
        rel = os.path.relpath(p, root)
        st = os.lstat(p)
        e = {"mode": oct(stat.S_IMODE(st.st_mode)), "uid": st.st_uid, "gid": st.st_gid}
        if stat.S_ISLNK(st.st_mode):
            e["type"], e["target"] = "symlink", os.readlink(p)
        elif stat.S_ISDIR(st.st_mode):
            e["type"] = "dir"
        else:
            e["type"], e["size"], e["mtimeNs"] = "file", st.st_size, st.st_mtime_ns
            with open(p, "rb") as f:
                e["sha256"] = hashlib.sha256(f.read()).hexdigest()
            if st.st_nlink > 1:
                e["linkGroup"] = inodes.setdefault(st.st_ino, rel)
        entries[rel] = e
checks = {}
try:
    checks["sqlite"] = sqlite3.connect(os.path.join(root, "app.db")).execute("pragma integrity_check").fetchone()[0]
except Exception as error:
    checks["sqlite"] = repr(error)
fsck = subprocess.run(["git", "-C", os.path.join(root, "repo"), "fsck", "--strict"], capture_output=True, text=True,
                      env=dict(os.environ, HOME="/home/app"))
checks["gitFsck"] = fsck.returncode
checks["gitFsckOutput"] = (fsck.stdout + fsck.stderr)[-500:]
print(json.dumps({"entries": entries, "checks": checks}, sort_keys=True))
