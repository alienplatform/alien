"""Change a running fixture's network and prove Resume rechecks the actual paths."""
import json


def check_resume_path(suite, name, call, docker, networks, prefix, metadata):
    suffix = "metadata-canary" if metadata else "udp-canary"
    subnet, origin, guest = ("169.254.169.0/24", "169.254.169.254", "169.254.169.10") if metadata else ("8.8.8.0/24", "8.8.8.8", "8.8.8.10")
    network = prefix+"-"+suffix
    docker("network","create","--internal","--subnet",subnet,network)
    networks.append(network)
    docker("network","connect","--ip",origin,network,prefix+"-origin")
    docker("network","connect","--ip",guest,network,name)
    if metadata:
        control = 'import urllib.request;assert urllib.request.urlopen("http://169.254.169.254/",timeout=2).read()==b"controlled-origin-response\\n"'
    else:
        control = 'import socket; s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);s.settimeout(2);s.sendto(bytes.fromhex("002a010000010000000000000665677265737307696e76616c69640000010001"),("8.8.8.8",53));assert s.recv(512)[-4:]==bytes([93,184,216,2])'
    def fail_closed():
        assert docker("exec",name,"python","-c",control)==""
        status,body=call("POST","/aws/lambda-microvms/runtime/v1/resume",{})
        assert status==503,(status,body)
        status,body=call("POST","/v1/exec",{"command":["true"],"timeoutMs":1000})
        assert status==503,(status,body)
        assert call("GET","/v1/health")[0]==503
    try:
        suite.record(f"Resume detects reachable {suffix} and gates commands",fail_closed)
    finally:
        docker("network","disconnect",network,name)
        docker("network","disconnect",network,prefix+"-origin")
    def recover():
        status,body=call("POST","/aws/lambda-microvms/runtime/v1/resume",{})
        assert status==200,(status,body)
        status,body=call("GET","/v1/health")
        assert status==200 and json.loads(body)["status"]=="ready",(status,body)
    suite.record(f"Resume recovers after closing {suffix}",recover)
