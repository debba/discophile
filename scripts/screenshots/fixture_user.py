"""Creates the iOS validation build's fixture-user on the demo servers, with the demo user's playlists, favorites
and audiobook progress, so iPhone screenshots show the same library state as the desktop ones."""
import hashlib, json, random, string, time, urllib.parse, urllib.request

ND, ABS = "http://localhost:4542", "http://localhost:13382"
USER, PASSWORD = "fixture-user", "fixture-password"


def call(url, body=None, headers={}, method=None):
    req = urllib.request.Request(url, data=json.dumps(body).encode() if body is not None else None, method=method,
                                 headers={"Content-Type": "application/json", **headers})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            data = r.read()
            return json.loads(data) if data[:1] in (b"{", b"[") else data.decode()
    except urllib.error.HTTPError as e:
        return {"error": e.code, "body": e.read().decode()[:200]}


def subsonic(user, password, endpoint, **params):
    salt = "".join(random.choices(string.ascii_lowercase, k=8))
    query = {"u": user, "t": hashlib.md5((password + salt).encode()).hexdigest(), "s": salt, "v": "1.16.1", "c": "demo", "f": "json"}
    pairs = list(query.items()) + [(k, x) for k, v in params.items() for x in (v if isinstance(v, list) else [v])]
    return call(f"{ND}/rest/{endpoint}?{urllib.parse.urlencode(pairs)}")["subsonic-response"]


jwt = call(ND + "/auth/login", {"username": "admin", "password": "demo-screens"})["token"]
print("navidrome user", call(ND + "/api/user", {"userName": USER, "name": "Alex", "password": PASSWORD, "isAdmin": True},
                             {"x-nd-authorization": "Bearer " + jwt}))

# Same playlists and favorites as the demo user.
existing = {p["name"] for p in subsonic(USER, PASSWORD, "getPlaylists").get("playlists", {}).get("playlist", [])}
for playlist in subsonic("demo", "demo-password", "getPlaylists")["playlists"]["playlist"]:
    if playlist["name"] in existing:
        continue
    songs = subsonic("demo", "demo-password", "getPlaylist", id=playlist["id"])["playlist"].get("entry", [])
    subsonic(USER, PASSWORD, "createPlaylist", name=playlist["name"], songId=[s["id"] for s in songs])
    print("playlist", playlist["name"], len(songs))
starred = subsonic("demo", "demo-password", "getStarred2")["starred2"].get("song", [])
subsonic(USER, PASSWORD, "star", id=[s["id"] for s in starred])
print("favorites", len(starred))

token = call(ABS + "/login", {"username": "root", "password": "demo-screens"})["user"]["token"]
print("abs user", str(call(ABS + "/api/users", {"username": USER, "password": PASSWORD, "type": "user", "isActive": True,
      "permissions": {"download": True, "update": False, "delete": False, "upload": False, "accessAllLibraries": True,
                      "accessAllTags": True}}, {"Authorization": "Bearer " + token}))[:70])
