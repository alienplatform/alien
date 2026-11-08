#!/bin/sh
# Exercise the exact valkey-server binary being packaged: password required, writes survive a
# restart with the append-only file on, and are gone with persistence off.
set -eu
server=$1
cli=$2
dir=$(mktemp -d)
port=56379
cleanup() {
  [ -f "$dir/pid" ] && kill "$(cat "$dir/pid")" 2>/dev/null || true
  rm -rf "$dir"
}
trap cleanup EXIT

start() {
  "$server" --port "$port" --bind 127.0.0.1 --requirepass testpass --dir "$dir" \
    --appendonly "$1" --appendfsync everysec --save "" --daemonize no >"$dir/server.log" 2>&1 &
  echo $! >"$dir/pid"
  for _ in $(seq 1 50); do
    "$cli" -p "$port" --no-auth-warning -a testpass PING >/dev/null 2>&1 && return 0
    sleep 0.1
  done
  cat "$dir/server.log" >&2
  return 1
}
stop() {
  kill "$(cat "$dir/pid")"
  wait "$(cat "$dir/pid")" 2>/dev/null || true
  rm -f "$dir/pid"
}
run() { "$cli" -p "$port" --no-auth-warning -a testpass "$@"; }

start yes
test "$("$cli" -p "$port" PING 2>&1 | head -c 8)" = "NOAUTH A" # no password, no access
run SET k v >/dev/null
run EVAL "redis.call('set',KEYS[1],1);redis.call('set',KEYS[2],2);return 1" 2 a b >/dev/null
stop
start yes
test "$(run GET k)" = "v"
stop

rm -rf "$dir"/appendonlydir
start no
run SET k v >/dev/null
stop
start no
test -z "$(run GET k)"
echo "valkey smoke test passed"
