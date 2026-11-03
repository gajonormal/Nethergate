"""Read-only smoke checks for a running development hub (standard library only)."""
import json
import urllib.error
import urllib.request

BASE = "http://127.0.0.1:17843"


def request(path, payload=None, headers=None):
    data = None if payload is None else json.dumps(payload).encode()
    values = {"Content-Type": "application/json", **(headers or {})}
    query = urllib.request.Request(BASE + path, data=data, headers=values)
    try:
        with urllib.request.urlopen(query, timeout=10) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.read()


if __name__ == "__main__":
    code, body = request("/")
    assert code == 200 and b'<div id="root">' in body
    code, _ = request("/api/call", {"action": "list"})
    assert code == 403, "API accepted an unauthenticated request"
    code, _ = request("/api/session", headers={"X-NetherGate-Client": "hub", "Origin": "https://example.org"})
    assert code == 403, "API accepted an external origin"
    code, _ = request("/api/session", headers={"X-NetherGate-Client": "hub", "Host": "example.org"})
    assert code == 403, "API accepted an untrusted Host header"
    code, body = request("/api/session", headers={"X-NetherGate-Client": "hub"})
    assert code == 200
    token = json.loads(body)["token"]
    auth = {"X-NetherGate-Token": token}
    code, body = request("/api/call", {"action": "list", "payload": {}}, auth)
    assert code == 200 and isinstance(json.loads(body), list)
    code, _ = request("/api/call", {"action": "logs", "payload": {"id": "../secret"}}, auth)
    assert code == 400, "API accepted an unknown server identifier"
    code, _ = request("/../Cargo.toml")
    assert code == 403, "Static file handler allowed path traversal"
    print("8 verificações aprovadas: interface, sessão, origem, Host, autenticação, listagem, IDs e caminhos.")
