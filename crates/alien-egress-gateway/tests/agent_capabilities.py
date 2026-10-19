"""Measure the supervisor and child capability boundary using real processes."""
import base64
import json

# The image API on AWS accepts only ALL. These are the capabilities the agent
# retains before starting threads; Docker can also constrain its initial grant.
SUPERVISOR_CAPABILITIES = ["CHOWN", "DAC_OVERRIDE", "FOWNER", "KILL", "SETGID", "SETUID", "SETPCAP", "NET_ADMIN"]
AGENT_CAP_ARGS = ["--cap-drop", "ALL", *[arg for cap in SUPERVISOR_CAPABILITIES for arg in ("--cap-add", cap)]]


def check_capabilities(suite, container, call, docker, uid):
    def threads():
        # Read each thread, since a reduction applied only after creating Tokio's
        # workers leaves those workers privileged even when the main thread is not.
        program = '''from pathlib import Path
statuses=list(Path('/proc/1/task').glob('*/status'))
assert len(statuses)>1
for path in statuses:
 values=dict(line.split(':',1) for line in path.read_text().splitlines() if ':' in line)
 for field in ['CapEff','CapPrm','CapBnd']: assert int(values[field],16)==0x11eb,(path,field,values[field])
 for field in ['CapInh','CapAmb']: assert int(values[field],16)==0,(path,field,values[field])
 assert int(values['NoNewPrivs'])==1
print('every supervisor thread has exactly eight capabilities')'''
        assert docker("exec", container, "python", "-c", program) == "every supervisor thread has exactly eight capabilities"
    suite.record("every supervisor thread retains only the measured eight capabilities", threads)

    def execute(program, timeout=5000):
        status, body = call("POST", "/v1/exec", {"command": ["python", "-c", program], "timeoutMs": timeout})
        assert status == 200, (status, body)
        frames = [json.loads(line) for line in body.splitlines()]
        output = b"".join(base64.b64decode(f["data"]) for f in frames if f["t"] in ["stdout", "stderr"])
        return frames[-1], output

    def child():
        terminal, output = execute(f'''import os,socket
from pathlib import Path
assert os.getuid()=={uid} and os.getgid()=={uid} and os.getgroups()==[]
values=dict(line.split(':',1) for line in Path('/proc/self/status').read_text().splitlines() if ':' in line)
for field in ['CapEff','CapPrm','CapBnd','CapInh','CapAmb']: assert int(values[field],16)==0,(field,values[field])
assert int(values['NoNewPrivs'])==1
try: os.setuid(0)
except PermissionError: pass
else: raise AssertionError('uid drop was reversible')
try: socket.socket(socket.AF_INET,socket.SOCK_RAW,socket.IPPROTO_TCP)
except PermissionError: pass
else: raise AssertionError('raw socket allowed')
p=Path('/data/private-capability-file');p.write_text('guest-owned');p.chmod(0)
''')
        assert terminal["t"] == "exit" and terminal["code"] == 0, (terminal, output)
        status, body = call("GET", "/v1/files?path=private-capability-file")
        assert status == 200 and base64.b64decode(json.loads(body)["contentsBase64"]) == b"guest-owned", (status, body)
        status, body = call("PUT", "/v1/files", {"path": "private-capability-file", "contentsBase64": base64.b64encode(b"agent-write").decode()})
        assert status == 204, (status, body)
    suite.record("exec drops every capability and agent can transfer mode-zero guest files", child)

    def cancellation():
        terminal, output = execute("import os,time;open('/data/cap-timeout-pid','w').write(str(os.getpid()));time.sleep(30)", 300)
        assert terminal["t"] == "error" and terminal["code"] == "timeoutExceeded", (terminal, output)
        # The API completes only after killing and reaping the unprivileged child.
        assert docker("exec", container, "python", "-c", "from pathlib import Path; pid=Path('/data/cap-timeout-pid').read_text(); assert not Path('/proc/'+pid).exists()") == ""
    suite.record("reduced supervisor kills and reaps a timed-out command", cancellation)
