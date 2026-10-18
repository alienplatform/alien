"""Linux host-only identity witness; emit a token fingerprint, never packet contents."""

import argparse
import base64
import hashlib
import json
import socket
import struct
import sys
import time


def fingerprint_packet(packet, audience, sandbox_id, port):
    # AF_PACKET's Ethernet header is present on Linux loopback as well as veth.
    if len(packet) < 54 or packet[12:14] != b"\x08\x00":
        return None
    ip = packet[14:]
    if ip[0] >> 4 != 4 or ip[9] != 6 or struct.unpack("!H", ip[6:8])[0] & 0x3FFF:
        return None
    ip_size = (ip[0] & 15) * 4
    tcp = ip[ip_size : struct.unpack("!H", ip[2:4])[0]]
    if ip_size < 20 or len(tcp) < 20 or struct.unpack("!H", tcp[2:4])[0] != port:
        return None
    tcp_size = (tcp[12] >> 4) * 4
    if tcp_size < 20:
        return None
    head, separator, _body = tcp[tcp_size:].partition(b"\r\n\r\n")
    if not separator or not head.startswith(b"CONNECT ") or len(head) > 8192:
        return None
    for line in head.split(b"\r\n")[1:]:
        name, colon, value = line.partition(b":")
        if not colon or name.lower() != b"proxy-authorization":
            continue
        scheme, space, token = value.strip().partition(b" ")
        if scheme.lower() != b"bearer" or not space or not 1 <= len(token) <= 4096:
            return None
        parts = token.split(b".")
        if len(parts) != 3:
            return None
        try:
            claims = json.loads(
                base64.urlsafe_b64decode(parts[1] + b"=" * (-len(parts[1]) % 4))
            )
        except (ValueError, UnicodeDecodeError):
            return None
        if not isinstance(claims, dict) or (
            claims.get("iss"),
            claims.get("aud"),
            claims.get("sub"),
        ) != ("alien-sandbox", audience, sandbox_id):
            return None
        return hashlib.sha256(token).hexdigest()
    return None


def capture(args):
    # Attach only on the trusted agent/gateway host. No pcap or raw packet logging.
    with socket.socket(
        socket.AF_PACKET, socket.SOCK_RAW, socket.htons(0x0800)
    ) as packets:
        if args.interface:
            packets.bind((args.interface, 0))
        deadline = time.monotonic() + args.timeout
        while time.monotonic() < deadline:
            packets.settimeout(max(0.001, deadline - time.monotonic()))
            try:
                packet = packets.recv(65535)
            except TimeoutError:
                return None
            result = fingerprint_packet(
                packet, args.audience, args.sandbox_id, args.port
            )
            if result:
                return result
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--audience", required=True)
    parser.add_argument("--sandbox-id", required=True)
    parser.add_argument(
        "--interface", help="host interface carrying the gateway connection"
    )
    parser.add_argument("--port", type=int, default=3128)
    parser.add_argument("--timeout", type=float, default=8)
    args = parser.parse_args()
    if not 0 < args.timeout <= 8 or not 1 <= args.port <= 65535:
        parser.error("timeout must be in (0, 8] and port in [1, 65535]")
    try:
        result = capture(args)
    except OSError:
        # Capability and interface errors must not dump any packet or credential.
        return 1
    if not result:
        return 1
    print(result)
    return 0


if __name__ == "__main__":
    sys.exit(main())
