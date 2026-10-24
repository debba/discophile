"""Marks a few demo audiobooks as in progress for the demo user."""
import json, urllib.request

ABS = "http://localhost:13382"
PROGRESS = {"Orbit Zero": 0.38, "The Long Water": 0.12, "Interlude": 0.71, "Ember Games": 0.05}


def call(path, body=None, token=None, method=None):
    headers = {"Content-Type": "application/json", **({"Authorization": "Bearer " + token} if token else {})}
    req = urllib.request.Request(ABS + path, data=json.dumps(body).encode() if body is not None else None, headers=headers, method=method)
    with urllib.request.urlopen(req, timeout=30) as r:
        data = r.read()
        return json.loads(data) if data[:1] in (b"{", b"[") else None


import sys
USER = sys.argv[1] if len(sys.argv) > 1 else "demo"
PASSWORD = sys.argv[2] if len(sys.argv) > 2 else "demo-password"
token = call("/login", {"username": USER, "password": PASSWORD})["user"]["token"]
library = call("/api/libraries", token=token)["libraries"][0]["id"]
for item in call(f"/api/libraries/{library}/items?limit=100", token=token)["results"]:
    title = item["media"]["metadata"]["title"]
    if title in PROGRESS:
        duration = item["media"]["duration"]
        call(f"/api/me/progress/{item['id']}", {"duration": duration, "currentTime": duration * PROGRESS[title],
                                               "progress": PROGRESS[title], "isFinished": False}, token, "PATCH")
        print(title, PROGRESS[title])
