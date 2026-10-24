#!/bin/sh
# Exercise the exact distribution and extension being packaged, as an unprivileged user.
set -eu
bindir=$("$PG_CONFIG" --bindir)
sharedir=$("$PG_CONFIG" --sharedir)
test_dir=$(mktemp -d)
cleanup() {
  if [ -f "$test_dir/data/postmaster.pid" ]; then
    "$bindir/pg_ctl" -D "$test_dir/data" -m immediate -w stop
  fi
  rm -rf "$test_dir"
}
trap cleanup EXIT
"$bindir/initdb" -D "$test_dir/data" -L "$sharedir" --auth=trust --no-locale --encoding=UTF8
start() {
  "$bindir/pg_ctl" -D "$test_dir/data" -l "$test_dir/server.log" \
    -o "-h 127.0.0.1 -p 55432 -k $test_dir" -w start
}
query() {
  "$bindir/psql" -h "$test_dir" -p 55432 -d postgres -v ON_ERROR_STOP=1 "$@"
}
start
query <<'SQL'
SET TIME ZONE 'UTC';
CREATE EXTENSION vector;
CREATE TABLE vectors (id integer PRIMARY KEY, embedding vector(3));
INSERT INTO vectors VALUES (1, '[1,0,0]'), (2, '[0,1,0]'), (3, '[0,0,1]');
CREATE INDEX ON vectors USING hnsw (embedding vector_l2_ops);
SQL
"$bindir/pg_ctl" -D "$test_dir/data" -w stop
start
query <<'SQL'
SET enable_seqscan = off;
SELECT 1 / ((SELECT id FROM vectors ORDER BY embedding <-> '[0.9,0,0]' LIMIT 1) = 1)::integer;
SQL
"$bindir/pg_ctl" -D "$test_dir/data" -w stop
