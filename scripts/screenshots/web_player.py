"""Captures the web player for the website's Download page, light and dark, in a real (headless) Chromium.
Needs the demo servers, the desktop demo profile in .screenshots/profile, Playwright and Pillow, and the web player:
  docker run -d --name dweb --network discophile-screens_default -p 8090:8080 \\
    -e DISCOPHILE_PROXY_NAVIDROME=http://navidrome:4533 -e DISCOPHILE_PROXY_AUDIOBOOKSHELF=http://audiobookshelf:80 \\
    ghcr.io/debba/discophile-web:latest
The desktop profile is copied into the browser's storage, pointed at the player's same-origin proxies."""
import hashlib, json, pathlib
from PIL import Image
from playwright.sync_api import sync_playwright

ROOT = pathlib.Path(__file__).resolve().parents[2]
STATE = ROOT / ".screenshots/profile/state"
OUT = ROOT / "site/public/assets/screens/web"
PLAYER = "http://localhost:8090"


def key(name):
    return "discophile:" + "/".join("".join(c if c.isalnum() or c in "-_." else "_" for c in part) for part in name.split("/") if part)


def storage(appearance):
    accounts = json.loads((STATE / "accounts.json").read_text())
    old = accounts["active"]
    account = accounts["accounts"][0]
    account["credentials"]["server"] = f"{PLAYER}/proxy/navidrome"
    new = hashlib.md5(f"{account['credentials']['server']}\n{account['credentials']['username']}".encode()).hexdigest()
    accounts["active"] = new
    secrets = json.loads((ROOT / ".screenshots/profile/secrets-local.json").read_text())
    items = {key("accounts"): accounts, key(f"secrets/account:{new}"): secrets[f"account:{old}"],
             key("secrets/audiobookshelf-tokens"): secrets["audiobookshelf-tokens"]}
    for document in (STATE / "library" / old).glob("*.json"):
        items[key(f"library/{new}/{document.stem}")] = json.loads(document.read_text())
    books = json.loads((STATE / "audiobooks/account.json").read_text())
    books["server"] = f"{PLAYER}/proxy/audiobookshelf"
    items[key("audiobooks/account")] = books
    settings = json.loads((STATE / "settings.json").read_text())
    settings.update(appearance=appearance, check_for_updates=False)
    items[key("settings")] = settings
    return {k: json.dumps(v) for k, v in items.items()}


with sync_playwright() as p:
    browser = p.chromium.launch()
    for mode, folder in (("dark", OUT), ("light", OUT / "light")):
        folder.mkdir(parents=True, exist_ok=True)
        # Device scale 1: egui would scale the interface twice at 2x.
        context = browser.new_context(viewport={"width": 1280, "height": 800}, device_scale_factor=1, color_scheme=mode)
        context.add_init_script(f"if(!sessionStorage.seeded){{const s={json.dumps(storage(mode.title()))};"
                                "for(const k in s) localStorage.setItem(k,s[k]); sessionStorage.seeded=1}")
        page = context.new_page()
        page.goto(PLAYER, wait_until="networkidle")
        page.wait_for_timeout(6000)
        page.mouse.click(76, 363)  # Albums in the sidebar
        page.wait_for_timeout(3500)
        page.mouse.move(1279, 400)
        page.wait_for_timeout(800)
        page.screenshot(path="/tmp/web-player.png")
        Image.open("/tmp/web-player.png").convert("RGB").save(folder / "albums.webp", "WEBP", quality=88, method=6)
        context.close()
    browser.close()
print("saved", OUT)
