"""HTTP and WebSocket on 8080, 8081 and 8090. Every request needs the bearer token.

A 200 reply carries the SHA-256 of the Authorization header the server received, so a
caller can prove it arrived unchanged. WebSocket messages are echoed back unchanged.
"""
import asyncio
import base64
import hashlib
import json
import os
import struct
import subprocess
import sys

TOKEN = os.environ.get("ALIEN_CONFORMANCE_TOKEN", "conformance-bearer-token")
PORTS = (8080, 8081, 8090)
MAX_MESSAGE = 8 * 1024 * 1024
MAX_BODY = 8 * 1024 * 1024
GUID = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


def reason(status):
    return {101: "Switching Protocols", 200: "OK", 400: "Bad Request",
            401: "Unauthorized", 404: "Not Found"}[status]


async def reply(writer, status, body=b"", headers=()):
    head = [f"HTTP/1.1 {status} {reason(status)}", f"Content-Length: {len(body)}",
            "Connection: close"] + list(headers)
    writer.write(("\r\n".join(head) + "\r\n\r\n").encode() + body)
    await writer.drain()


async def read_frame(reader):
    first, second = await reader.readexactly(2)
    length = second & 0x7F
    if length == 126:
        length = struct.unpack(">H", await reader.readexactly(2))[0]
    elif length == 127:
        length = struct.unpack(">Q", await reader.readexactly(8))[0]
    if length > MAX_MESSAGE:
        raise ValueError("frame too large")
    mask = await reader.readexactly(4) if second & 0x80 else None
    data = await reader.readexactly(length)
    if mask:
        data = bytes(byte ^ mask[index % 4] for index, byte in enumerate(data))
    return first & 0x0F, bool(first & 0x80), data


def frame(opcode, data):
    head = bytes([0x80 | opcode])
    if len(data) < 126:
        head += bytes([len(data)])
    elif len(data) < 65536:
        head += b"\x7e" + struct.pack(">H", len(data))
    else:
        head += b"\x7f" + struct.pack(">Q", len(data))
    return head + data


async def websocket(reader, writer, headers):
    accept = base64.b64encode(hashlib.sha1(headers["sec-websocket-key"].encode() + GUID).digest())
    writer.write(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n"
                 b"Connection: Upgrade\r\nSec-WebSocket-Accept: " + accept + b"\r\n\r\n")
    await writer.drain()
    message, kind = b"", 0
    while True:
        opcode, final, data = await read_frame(reader)
        if opcode == 8:
            writer.write(frame(8, data[:2]))
            await writer.drain()
            return
        if opcode == 9:
            writer.write(frame(10, data))
        elif opcode in (1, 2, 0):
            if opcode:
                kind, message = opcode, b""
            message += data
            if len(message) > MAX_MESSAGE:
                raise ValueError("message too large")
            if final:
                writer.write(frame(kind, message))
        await writer.drain()


async def handle(port, reader, writer):
    try:
        request = await reader.readuntil(b"\r\n\r\n")
        lines = request.decode("latin-1").split("\r\n")
        method, path, _ = lines[0].split(" ", 2)
        headers = {}
        for line in lines[1:]:
            if ":" in line:
                name, value = line.split(":", 1)
                headers[name.strip().lower()] = value.strip()
        authorization = headers.get("authorization", "")
        if authorization != f"Bearer {TOKEN}":
            await reply(writer, 401, b"bearer token required\n")
        elif headers.get("upgrade", "").lower() == "websocket":
            await websocket(reader, writer, headers)
        elif path.startswith("/whoami"):
            body = json.dumps({
                "port": port, "uid": os.getuid(),
                "authorizationSha256": hashlib.sha256(authorization.encode()).hexdigest(),
            }).encode()
            await reply(writer, 200, body, ["Content-Type: application/json"])
        elif path.startswith("/metadata/"):
            # Runs the credential probe as this server's own identity (the workload uid).
            backend = path.rsplit("/", 1)[1]
            done = await asyncio.to_thread(
                subprocess.run,
                [sys.executable, "/opt/contract-conformance/probe_metadata.py", backend],
                capture_output=True, text=True, timeout=60)
            await reply(writer, 200, json.dumps({
                "uid": os.getuid(), "exitCode": done.returncode,
                "report": done.stdout[-60000:], "failure": done.stderr[-2000:]}).encode(),
                ["Content-Type: application/json"])
        elif path.startswith("/echo"):
            length = int(headers.get("content-length", "0"))
            if length > MAX_BODY:
                await reply(writer, 400, b"body too large\n")
            else:
                body = await reader.readexactly(length)
                await reply(writer, 200, json.dumps({
                    "port": port, "bytes": len(body), "method": method,
                    "sha256": hashlib.sha256(body).hexdigest(),
                }).encode(), ["Content-Type: application/json"])
        else:
            await reply(writer, 404, b"not found\n")
    except (asyncio.IncompleteReadError, ConnectionError, ValueError):
        pass  # A peer that hangs up or misbehaves only ends its own connection.
    finally:
        writer.close()


async def main():
    servers = [await asyncio.start_server(lambda r, w, p=port: handle(p, r, w), "0.0.0.0", port,
                                          limit=MAX_MESSAGE + 1024, backlog=256)
               for port in PORTS]
    await asyncio.gather(*(server.serve_forever() for server in servers))


if __name__ == "__main__":
    asyncio.run(main())
