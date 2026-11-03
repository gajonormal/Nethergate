"""Query the local Minecraft status protocol without logging in or joining the world."""

import argparse
import json
import socket
import statistics
import struct
import time


def varint(value):
    result = bytearray()
    while value > 127:
        result.append((value & 127) | 128)
        value >>= 7
    result.append(value)
    return bytes(result)


def receive(sock, length):
    result = bytearray()
    while len(result) < length:
        chunk = sock.recv(length - len(result))
        if not chunk:
            raise ConnectionError("Server closed the status connection")
        result.extend(chunk)
    return bytes(result)


def read_varint(sock):
    result = 0
    for shift in range(0, 35, 7):
        byte = receive(sock, 1)[0]
        result |= (byte & 127) << shift
        if not byte & 128:
            return result
    raise ValueError("Invalid Minecraft VarInt")


def status(host, port, timings=False):
    address = host.encode("utf-8")
    # Protocol 47 is used only for discovery; the response reports the actual version.
    handshake = b"\x00" + varint(47) + varint(len(address)) + address
    handshake += struct.pack(">H", port) + b"\x01"
    start = time.perf_counter()
    with socket.create_connection((host, port), timeout=10) as sock:
        connected = time.perf_counter()
        sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        sock.sendall(varint(len(handshake)) + handshake + b"\x01\x00")
        packet_length = read_varint(sock)
        if packet_length > 2_097_152:
            raise ValueError("Status response is too large")
        if read_varint(sock) != 0:
            raise ValueError("Unexpected status packet")
        length = read_varint(sock)
        if length > packet_length or length > 2_097_152:
            raise ValueError("Invalid status payload length")
        result = json.loads(receive(sock, length).decode("utf-8"))
        received = time.perf_counter()
        if not timings:
            return result
        payload = struct.pack(">q", time.monotonic_ns())
        ping_start = time.perf_counter()
        sock.sendall(b"\x09\x01" + payload)
        if read_varint(sock) != 9 or read_varint(sock) != 1:
            raise ValueError("Unexpected Minecraft pong packet")
        if receive(sock, 8) != payload:
            raise ValueError("Minecraft pong payload differs from ping")
        return {
            "status": result,
            "connect_ms": (connected - start) * 1000,
            "status_ms": (received - connected) * 1000,
            "ping_ms": (time.perf_counter() - ping_start) * 1000,
        }


def benchmark(host, port, samples):
    measurements = []
    errors = []
    for sample in range(samples):
        try:
            measurements.append(status(host, port, timings=True))
        except (OSError, ValueError, ConnectionError) as error:
            errors.append(str(error))
        if sample + 1 < samples:
            time.sleep(0.5)
    result = {"requested": samples, "successful": len(measurements), "errors": errors}
    if measurements:
        result["server"] = measurements[-1]["status"]
        for metric in ("connect_ms", "status_ms", "ping_ms"):
            values = [m[metric] for m in measurements]
            result[metric] = {
                "min": round(min(values), 2),
                "median": round(statistics.median(values), 2),
                "max": round(max(values), 2),
                "samples": [round(value, 2) for value in values],
            }
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=25565)
    parser.add_argument("--samples", type=int, default=0, help="Measure status and ping/pong RTT (1-30 samples)")
    args = parser.parse_args()
    if not 0 <= args.samples <= 30:
        parser.error("--samples must be between 0 and 30")
    result = benchmark(args.host, args.port, args.samples) if args.samples else status(args.host, args.port)
    print(json.dumps(result, ensure_ascii=False, indent=2))
