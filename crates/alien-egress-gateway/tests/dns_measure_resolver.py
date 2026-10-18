"""Controlled UDP/TCP resolver that records fake query names; never forwards queries."""
import json
import socket
import struct
import threading
import time
from pathlib import Path

lock = threading.Lock()


def answer(packet, transport):
    offset, labels = 12, []
    while packet[offset]:
        length = packet[offset]
        labels.append(packet[offset + 1:offset + 1 + length].decode("ascii"))
        offset += length + 1
    question = packet[12:offset + 5]
    with lock, open("/measure/queries.jsonl", "a") as log:
        log.write(json.dumps({"transport": transport, "name": ".".join(labels),
                              "queryBytes": len(packet), "time": time.monotonic()}) + "\n")
    # NXDOMAIN with the original question; this resolver never contacts an external service.
    return packet[:2] + struct.pack("!HHHHH", 0x8183, 1, 0, 0, 0) + question


def receive(stream, count):
    result = bytearray()
    while len(result) < count:
        data = stream.recv(count - len(result))
        if not data:
            raise EOFError("incomplete DNS frame")
        result.extend(data)
    return bytes(result)


def tcp_client(stream):
    with stream:
        stream.settimeout(3)
        packet = receive(stream, struct.unpack("!H", receive(stream, 2))[0])
        response = answer(packet, "tcp")
        stream.sendall(struct.pack("!H", len(response)) + response)


udp = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
udp.bind(("0.0.0.0", 53))
tcp = socket.socket()
tcp.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
tcp.bind(("0.0.0.0", 53))
tcp.listen()


def tcp_loop():
    while True:
        stream, _ = tcp.accept()
        threading.Thread(target=tcp_client, args=(stream,), daemon=True).start()


threading.Thread(target=tcp_loop, daemon=True).start()
Path("/measure/ready").touch()
while True:
    packet, sender = udp.recvfrom(4096)
    udp.sendto(answer(packet, "udp"), sender)
