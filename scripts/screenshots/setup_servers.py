"""Creates the demo user on both servers, the audiobook library, and scans. Idempotent."""
import json, time, urllib.error, urllib.request

ND, ABS = "http://localhost:4542", "http://localhost:13382"
USER, PASSWORD = "demo", "demo-password"


def call(url, body=None, headers={}, method=None):
    req = urllib.request.Request(url, data=json.dumps(body).encode() if body is not None else None, method=method,
                                 headers={"Content-Type": "application/json", **headers})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            data = r.read()
            return json.loads(data) if data[:1] in (b"{", b"[") else data.decode()
    except urllib.error.HTTPError as e:
        return {"error": e.code, "body": e.read().decode()[:200]}


for _ in range(60):
    try:
        urllib.request.urlopen(ND + "/ping", timeout=3); urllib.request.urlopen(ABS + "/status", timeout=3); break
    except Exception:
        time.sleep(2)
jwt = call(ND + "/auth/login", {"username": "admin", "password": "demo-screens"})["token"]
print("navidrome user", call(ND + "/api/user", {"userName": USER, "name": "Alex", "password": PASSWORD, "isAdmin": True},
                             {"x-nd-authorization": "Bearer " + jwt}))
print("navidrome scan", call(ND + "/rest/startScan?u=admin&p=demo-screens&v=1.16.1&c=demo&f=json&fullScan=true"))
if not call(ABS + "/status")["isInit"]:
    call(ABS + "/init", {"newRoot": {"username": "root", "password": "demo-screens"}})
token = call(ABS + "/login", {"username": "root", "password": "demo-screens"})["user"]["token"]; auth = {"Authorization": "Bearer " + token}
print("abs user", str(call(ABS + "/api/users", {"username": USER, "password": PASSWORD, "type": "user", "isActive": True,
      "permissions": {"download": True, "update": False, "delete": False, "upload": False, "accessAllLibraries": True, "accessAllTags": True}}, auth))[:80])
libs = call(ABS + "/api/libraries", headers=auth)["libraries"]
lib = libs[0]["id"] if libs else call(ABS + "/api/libraries", {"name": "Audiobooks", "folders": [{"fullPath": "/audiobooks"}],
                                                                 "mediaType": "book", "provider": "audible"}, auth)["id"]
call(ABS + f"/api/libraries/{lib}/scan?force=1", {}, auth); time.sleep(10)
print("abs books", call(ABS + f"/api/libraries/{lib}/items?limit=1", headers=auth)["total"])
