"""Opt-in IPv4 lookup integration test. Does not call the external port checker."""
import json
import urllib.error
import urllib.request

base = "http://127.0.0.1:17844"
with urllib.request.urlopen(urllib.request.Request(base + "/api/session", headers={"X-NetherGate-Client": "hub"}), timeout=10) as response:
    token = json.load(response)["token"]

def call(action, **payload):
    request = urllib.request.Request(base + "/api/call", data=json.dumps({"action": action, "payload": {"id": "fixture", **payload}}).encode(), headers={"Content-Type": "application/json", "X-NetherGate-Token": token})
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)

def rejected(action, **payload):
    try:
        call(action, **payload)
    except urllib.error.HTTPError as error:
        assert error.code == 400
        return
    raise AssertionError("Request should have been rejected")

rejected("internet_ip", consent=False)
lookup = call("internet_ip", consent=True)
assert lookup["source"] == "api.ipify.org"
rejected("internet_test", expectedIp=lookup["ip"], port=25591, consent=False)
rejected("internet_test", expectedIp="127.0.0.1", port=25591, consent=True)
comparison = call("internet_wan", wan="100.64.2.1")
assert comparison["state"] == "upstream_nat"
print(json.dumps({"result": "passed", "publicLookup": True, "consentEnforced": True, "forgedTargetRejected": True, "externalPortCheckerCalled": False}))
