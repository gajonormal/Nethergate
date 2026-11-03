"""Exercise the connection API against the disposable Paper fixture on localhost."""
import json
import socket
import time
import urllib.error
import urllib.request
from pathlib import Path

project = Path(__file__).resolve().parents[1]
root = project / "hub-data" / "players-ui-test-733b7613"
base = "http://127.0.0.1:17844"
request = urllib.request.Request(base + "/api/session", headers={"X-NetherGate-Client": "hub"})
with urllib.request.urlopen(request, timeout=10) as response:
    token = json.load(response)["token"]

def call(action, **payload):
    request = urllib.request.Request(base + "/api/call", data=json.dumps({"action": action, "payload": {"id": "fixture", **payload}}).encode(), headers={"Content-Type": "application/json", "X-NetherGate-Token": token})
    with urllib.request.urlopen(request, timeout=25) as response:
        return json.load(response)

def wait_for(state):
    until = time.monotonic() + 70
    while time.monotonic() < until:
        entry = call("list")[0]
        if entry["state"] == state:
            return entry
        if entry["state"] == "error":
            raise RuntimeError(entry["error"])
        time.sleep(0.5)
    raise TimeoutError(f"Server did not reach {state}")

def stop():
    entry = call("list")[0]
    if entry["state"] in ("online", "starting"):
        call("stop")
    if entry["state"] not in ("offline", "error"):
        wait_for("offline")

evidence = {}
try:
    initial = call("connection")
    assert initial["scope"] == "local", "Fixture must start in loopback"
    evidence["stopped"] = call("diagnose_connection")
    assert evidence["stopped"]["portState"] == "available"
    call("start")
    wait_for("online")
    evidence["local"] = call("diagnose_connection")
    assert evidence["local"]["probe"]["status"] == "minecraft"
    assert not any(a["configured"] for a in evidence["local"]["adapters"])
    try:
        call("set_connection", mode="local")
    except urllib.error.HTTPError as error:
        assert error.code == 400
        evidence["busyChangeRejected"] = True
    else:
        raise AssertionError("Must reject edits while Java runs")
    stop()
    if initial["adapters"]:
        ip = initial["adapters"][0]["ip"]
        call("set_connection", mode="lan", address=ip)
        entry = call("list")[0]
        assert entry["localAddress"] == f"{ip}:{entry['port']}"
        call("start")
        wait_for("online")
        evidence["lanFromThisPc"] = call("diagnose_connection")
        assert evidence["lanFromThisPc"]["scope"] == "lan"
        assert evidence["lanFromThisPc"]["probe"]["status"] == "minecraft"
        assert evidence["lanFromThisPc"]["probe"]["address"] == f"{ip}:25591"
        try:
            with socket.create_connection(("127.0.0.1", 25591), timeout=1):
                raise AssertionError("LAN-only binding must not advertise localhost")
        except OSError:
            evidence["localhostNotListeningInLanMode"] = True
    else:
        evidence["lanSkipped"] = "No supported Ethernet/Wi-Fi private address"
finally:
    stop()
    call("set_connection", mode="local")
    evidence["restored"] = call("connection")
    assert evidence["restored"]["scope"] == "local"
    (root / "connection-validation.json").write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding="utf-8")

assert all(value.get("internet") == "not_tested" and value.get("externalLan") == "not_tested"
           for value in evidence.values() if isinstance(value, dict) and "internet" in value)
print(json.dumps({"result": "passed", "local": evidence["local"]["probe"]["status"], "lan": evidence.get("lanFromThisPc", {}).get("probe", {}).get("status", "skipped"), "finalScope": evidence["restored"]["scope"], "evidence": str(root / "connection-validation.json")}, indent=2))
