"""Actual socket admission, cross-replica revocation, expiry and idle checks."""
import json
import struct
import socket
import threading
import time


def check_live_connections(suite, aud, origin, proxy, second, docker, prefix):
    policy = {"allow": [f"{origin}:2222"]}
    def install(sub, value=policy, replica=proxy):
        assert suite.admin("/v1/policies", {"aud": aud, "sub": sub, "policy": value}, proxy=replica)[0] == 204

    def connect(sub, replica=proxy, **claims):
        stream = suite.tunnel(origin, 2222, token=suite.token(sub=sub, **claims), proxy=replica)
        echo(stream)
        return stream

    def echo(stream):
        stream.settimeout(2)
        stream.sendall(b"ping")
        reply = b""
        while len(reply) < 4:
            part = stream.recv(4-len(reply))
            assert part, "echo closed"
            reply += part
        assert reply == b"ping", reply

    def closed(stream, deadline):
        until = time.monotonic()+deadline
        while time.monotonic() < until:
            try:
                echo(stream)
            except (OSError, AssertionError):
                return
            time.sleep(.2)
        raise AssertionError("existing tunnel remained open")

    def identity_capacity():
        install("capacity")
        install("neighbor")
        streams = []
        try:
            for _ in range(128):
                streams.append(connect("capacity"))
            try:
                excess = connect("capacity")
            except (ConnectionError, AssertionError, OSError):
                pass
            else:
                excess.close()
                raise AssertionError("identity cap not enforced")
            with connect("neighbor") as neighbor:
                echo(neighbor)
            streams.pop().close()
            time.sleep(.1)
            with connect("capacity") as recovered:
                echo(recovered)
        finally:
            for stream in streams:
                stream.close()
    suite.record("one identity cannot exhaust gateway capacity; slots recover", identity_capacity)

    def source_capacity():
        streams = []
        try:
            for _ in range(32):
                streams.append(socket.create_connection((proxy,3128),timeout=2))
            time.sleep(.1)
            with socket.create_connection((proxy,3128),timeout=2) as excess:
                excess.settimeout(1)
                try:
                    assert excess.recv(1) == b""
                except ConnectionResetError:
                    pass
            script = f'''import socket
s=socket.create_connection(({proxy!r},3128),timeout=2)
s.sendall(b"GET /health HTTP/1.1\\r\\nHost: gateway\\r\\nProxy-Authorization: Bearer {suite.token(sub="neighbor")}\\r\\n\\r\\n")
assert b"200 OK" in s.recv(1024)
s.close()
print("other source admitted")'''
            assert docker("exec", prefix+"-origin", "python", "-c", script) == "other source admitted"
        finally:
            for stream in streams:
                stream.close()
    suite.record("pre-auth source cap preserves admission from another IP", source_capacity)

    def dns_capacity():
        install("slow-dns", {"allow":["slow.example.test","allowed.test"]})
        streams=[]
        try:
            for _ in range(8):
                stream=socket.create_connection((proxy,3128),timeout=2)
                stream.sendall(f"CONNECT slow.example.test:443 HTTP/1.1\r\nHost: slow.example.test:443\r\nProxy-Authorization: Bearer {suite.token(sub='slow-dns')}\r\n\r\n".encode())
                streams.append(stream)
            time.sleep(.3)
            suite.tunnel(token=suite.token(sub="slow-dns"),expected=403)
            with connect("neighbor") as neighbor: echo(neighbor)
        finally:
            for stream in streams:stream.close()
    suite.record("stalled DNS is capped per identity and another sandbox proceeds",dns_capacity)

    def refusal_logs():
        def denied_count():
            status, metrics=suite.admin("/metrics",{},method="GET")
            assert status==200
            return int(next(line.split()[-1] for line in metrics.decode().splitlines() if line.startswith('alien_egress_denials_total{reason="authentication"}')))
        before=denied_count()
        logs_before=len(docker("logs",prefix+"-proxy1").splitlines())
        for _ in range(100): suite.tunnel(token="invalid",expected=403)
        assert denied_count()==before+100
        logs_after=len(docker("logs",prefix+"-proxy1").splitlines())
        assert logs_after-logs_before<100,(logs_before,logs_after)
    suite.record("refusal logs are rate limited while counters retain every denial",refusal_logs)

    def descriptor_exhaustion():
        install("descriptors")
        with connect("descriptors", second) as existing:
            def limit(code):
                return docker("run", "--rm", "--name", prefix+"-rlimit", "--pid", "container:"+prefix+"-proxy2", "--cap-add", "SYS_RESOURCE", "python:3.13-slim", "python", "-c", code)
            previous = json.loads(limit('import resource,json; old=resource.prlimit(1,resource.RLIMIT_NOFILE);resource.prlimit(1,resource.RLIMIT_NOFILE,(3,old[1]));print(json.dumps(old))'))
            pending=None
            try:
                pending=socket.create_connection((second,3128),timeout=2)
                time.sleep(.4)
                echo(existing)
                assert "os error 24" in docker("logs",prefix+"-proxy2")
            finally:
                limit(f'import resource;resource.prlimit(1,resource.RLIMIT_NOFILE,{tuple(previous)!r})')
            try:
                pending.sendall(f"GET /health HTTP/1.1\r\nHost: gateway\r\nProxy-Authorization: Bearer {suite.token(sub='descriptors')}\r\n\r\n".encode())
                assert b"200 OK" in pending.recv(1024)
            finally:
                if pending: pending.close()
    suite.record("EMFILE preserves existing tunnel and listener recovers", descriptor_exhaustion)

    def audit():
        install("audit-reset")
        stream=connect("audit-reset")
        stream.setsockopt(socket.SOL_SOCKET,socket.SO_LINGER,struct.pack("ii",1,0));stream.close()
        time.sleep(.1)
        logs=[json.loads(line) for line in docker("logs",prefix+"-proxy1").splitlines() if line.startswith("{")]
        events=[entry for entry in logs if entry.get("sub")=="audit-reset"]
        assert any(event.get("event")=="open" and event.get("decision")=="allow" for event in events),events
        assert any(event.get("event")=="close" and event.get("decision")=="allow" and "closeReason" in event for event in events),events
        status,metrics=suite.admin("/metrics",{},method="GET")
        assert status==200 and b"alien_egress_active_connections" in metrics and b'reason="capacity"' in metrics
        assert b"sub=" not in metrics and b"host=" not in metrics
    suite.record("allowed reset has separate close reason and bounded metric labels", audit)

    def revoke():
        install("revoke")
        with connect("revoke", second) as stream:
            install("revoke", {"allow": ["unrelated.example.test"]})
            closed(stream, 35)
        install("deleted")
        with connect("deleted", second) as stream:
            assert suite.admin("/v1/policies", {"aud":aud,"sub":"deleted"}, method="DELETE")[0] == 204
            closed(stream, 35)
    suite.record("existing replica tunnel closes after policy narrowing and deletion", revoke)

    def expiry():
        install("expires")
        with connect("expires", exp=int(time.time())+3) as stream:
            closed(stream,5)
    suite.record("existing tunnel closes at token expiry", expiry)

    def idle():
        install("idle")
        quiet, active = connect("idle"), connect("idle")
        stop = threading.Event()
        failures=[]
        def keepalive():
            while not stop.wait(1):
                try:
                    echo(active)
                except Exception as error:
                    failures.append(str(error))
                    return
        thread=threading.Thread(target=keepalive)
        thread.start()
        try:
            quiet.settimeout(124)
            assert quiet.recv(1) == b""
            assert not failures, failures
        finally:
            stop.set();thread.join();quiet.close();active.close()
    suite.record("120-second idle tunnel closes while active tunnel survives", idle)


def check_graceful_shutdown(suite, aud, origin, second, docker, prefix):
    def drain():
        assert suite.admin("/v1/policies",{"aud":aud,"sub":"drain","policy":{"allow":[f"{origin}:2222"]}})[0]==204
        with suite.tunnel(origin,2222,token=suite.token(sub="drain"),proxy=second) as stream:
            stream.sendall(b"before");assert stream.recv(6)==b"before"
            docker("kill","--signal","TERM",prefix+"-proxy2")
            time.sleep(.2)
            stream.sendall(b"during");assert stream.recv(6)==b"during"
            try:
                incoming=socket.create_connection((second,3128),timeout=1)
            except OSError:
                pass
            else:
                incoming.close();raise AssertionError("accepted during shutdown")
        deadline=time.monotonic()+17
        while docker("inspect","--format","{{.State.Running}}",prefix+"-proxy2")=="true":
            assert time.monotonic()<deadline,"shutdown exceeded drain deadline"
            time.sleep(.1)
        assert docker("inspect","--format","{{.State.ExitCode}}",prefix+"-proxy2")=="0"
    suite.record("SIGTERM stops accepting and drains existing tunnel",drain)
