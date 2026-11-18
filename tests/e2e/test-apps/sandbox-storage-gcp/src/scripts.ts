// Fixed shell programs the Worker runs in the sandbox. The endpoints are public, so only numbers
// from the caller reach them, each checked to be a non-negative integer first.

/** A digest of everything under /workspace except `tmp/`: path, type, mode, owner, link count,
 * mtime (not for symlinks, whose mtime a restore does not keep), symlink target and content. The
 * root and `lost+found` belong to each sandbox's own disk, not to the key, so they are left out. */
export const MANIFEST = `
set -eu
cd /workspace
{
  find . -mindepth 1 \\( -path ./tmp -o -path ./lost+found \\) -prune -o ! -type l -printf '%p %y %m %U %G %n %T@\\n'
  find . -mindepth 1 \\( -path ./tmp -o -path ./lost+found \\) -prune -o -type l -printf '%p %y %U %G %l\\n'
  find . -mindepth 1 \\( -path ./tmp -o -path ./lost+found \\) -prune -o -type f -print0 | xargs -0 -r sha256sum
} | LC_ALL=C sort > /tmp/manifest
printf 'entries=%s\\n' "$(find . -mindepth 1 \\( -path ./tmp -o -path ./lost+found \\) -prune -o -print | wc -l)"
printf 'bytes=%s\\n' "$(find . -mindepth 1 \\( -path ./tmp -o -path ./lost+found \\) -prune -o -type f -printf '%s\\n' | awk '{ s += $1 } END { print s + 0 }')"
printf 'manifest=%s\\n' "$(sha256sum < /tmp/manifest | cut -d' ' -f1)"
`

/** A tree with \`files\` small files, one random file of \`mib\` MiB, a hard link, symlinks, odd
 * modes, a git repository, a SQLite database in WAL mode and an excluded \`tmp/\` file. */
export function prepare(files: number, mib: number): string {
  return `
set -eu
cd /workspace
mkdir -p tree tmp
i=0
while [ "$i" -lt ${files} ]; do
  d="tree/d$((i % 100))"
  [ -d "$d" ] || mkdir -p "$d"
  printf 'file %s\\n' "$i" > "$d/f$i.txt"
  i=$((i + 1))
done
head -c $((${mib} * 1048576)) /dev/urandom > big.bin
printf 'linked\\n' > linked-a
ln -f linked-a linked-b
ln -sfn tree/d0 dir-link
ln -sfn missing-target dangling-link
printf '#!/bin/sh\\necho hi\\n' > run.sh
chmod 0750 run.sh
printf 'secret\\n' > private.txt
chmod 0600 private.txt
printf 'excluded\\n' > tmp/excluded
rm -rf repo
git init -q repo
git -C repo -c user.name=t -c user.email=t@t commit -q --allow-empty -m one
printf 'two\\n' > repo/two.txt
git -C repo add two.txt
git -C repo -c user.name=t -c user.email=t@t commit -q -m two
rm -f app.db app.db-wal app.db-shm
sqlite3 app.db 'PRAGMA journal_mode=WAL; CREATE TABLE t(v INTEGER); WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 1000) INSERT INTO t SELECT x FROM c;' > /dev/null
${MANIFEST}
`
}

/** Restored-side checks on top of the manifest: git, SQLite, the hard link and the exclusion. */
export const VERIFY = `
set -u
cd /workspace
${MANIFEST}
printf 'git_fsck=%s\\n' "$(git -C repo fsck --strict > /dev/null 2>&1 && echo ok || echo failed)"
printf 'sqlite=%s\\n' "$(sqlite3 app.db 'PRAGMA integrity_check; SELECT count(*) FROM t;' | tr '\\n' ' ')"
printf 'hard_link=%s\\n' "$([ linked-a -ef linked-b ] && echo same || echo split)"
printf 'excluded=%s\\n' "$([ -e tmp/excluded ] && echo present || echo absent)"
`

/** The kill-run writer: each round appends a numbered line and syncs it, inserts the number into a
 * SQLite WAL database, and renames a file over another; it prints the number once all three are
 * on the sandbox's disk. A git commit every tenth round keeps a repository changing too. */
export function writer(rounds: number): string {
  return `
set -eu
cd /workspace
mkdir -p churn
rm -f seq.log w.db w.db-wal w.db-shm
sqlite3 w.db 'PRAGMA journal_mode=WAL; CREATE TABLE t(v INTEGER);' > /dev/null
[ -d wrepo/.git ] || git init -q wrepo
i=0
while [ "$i" -lt ${rounds} ]; do
  i=$((i + 1))
  echo "$i" >> seq.log
  sync seq.log
  sqlite3 w.db "INSERT INTO t VALUES ($i);"
  printf '%s\\n' "$i" > churn/next
  mv -f churn/next churn/current
  if [ $((i % 10)) -eq 0 ]; then
    printf '%s\\n' "$i" > wrepo/n.txt
    git -C wrepo add n.txt
    git -C wrepo -c user.name=t -c user.email=t@t commit -q -m "$i"
  fi
  echo "$i"
  sleep 0.1
done
`
}

/** What a restore after a kill holds: the last line of the writer's log, and whether its database
 * and repository are intact. */
export const RESTORED = `
set -u
cd /workspace
printf 'last=%s\\n' "$(tail -n 1 seq.log 2>/dev/null || echo 0)"
printf 'sqlite=%s\\n' "$(sqlite3 w.db 'PRAGMA integrity_check;' 2>&1 | tr '\\n' ' ')"
printf 'sqlite_max=%s\\n' "$(sqlite3 w.db 'SELECT coalesce(max(v), 0) FROM t;' 2>&1)"
printf 'churn=%s\\n' "$(cat churn/current 2>/dev/null || echo none)"
printf 'git_fsck=%s\\n' "$( [ -d wrepo/.git ] && (git -C wrepo fsck --strict > /dev/null 2>&1 && echo ok || echo failed) || echo none)"
`

/** What the command's user can reach: credentials in its environment, the object storage API and
 * the metadata server's token (curl reports 000 when it gets no answer). */
export const ISOLATION = `
set -u
printf 'uid=%s\\n' "$(id -u)"
printf 'credential_env=%s\\n' "$(env | grep -ci -E 'GOOGLE|TOKEN|CREDENTIAL|SIGNED|BUCKET' || true)"
printf 'storage_api=%s\\n' "$(curl -sS -o /dev/null -m 10 -w '%{http_code}' https://storage.googleapis.com/storage/v1/b 2>/dev/null || true)"
printf 'metadata_token=%s\\n' "$(curl -sS -o /dev/null -m 5 -w '%{http_code}' -H 'Metadata-Flavor: Google' http://169.254.169.254/computeMetadata/v1/instance/service-accounts/default/token 2>/dev/null || true)"
mkdir -p /workspace/escape
ln -sfn / /workspace/escape/root
ln -sfn /proc/self /workspace/escape/proc
`

/** A tree above the declared 2 GiB, which the replicator must refuse whole. */
export const OVERSIZE = `
set -eu
cd /workspace
head -c 2306867200 /dev/zero > oversize.bin
sync oversize.bin
printf 'written=%s\\n' "$(stat -c %s oversize.bin)"
`
