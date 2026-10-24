"""Builds the silent, fully tagged demo library served by Navidrome and Audiobookshelf.
Usage: build_library.py WORK_DIR (covers are read from WORK_DIR/covers, written by covers.py)."""
import json, pathlib, re, subprocess, sys
from PIL import Image

HERE = pathlib.Path(__file__).resolve().parent
work = pathlib.Path(sys.argv[1]); catalog = json.load(open(HERE / "catalog.json"))
covers, music, books, tmp = work / "covers", work / "library/music", work / "library/audiobooks", work / "tmp"
for d in (music, books, tmp):
    d.mkdir(parents=True, exist_ok=True)


def run(*a): subprocess.run(["ffmpeg", "-v", "error", "-y", *a], check=True)
def safe(s): return re.sub(r'[\\/:*?"<>|]', "-", s).strip(". ")


def cover(src, dst):
    if not dst.exists():
        Image.open(src).convert("RGB").save(dst, quality=92)


silence_mp3, silence_m4a = tmp / "silence.mp3", tmp / "silence.m4a"
if not silence_mp3.exists():
    run("-f", "lavfi", "-i", "anullsrc=r=22050:cl=mono", "-t", "900", "-c:a", "libmp3lame", "-b:a", "16k", str(silence_mp3))
if not silence_m4a.exists():
    run("-f", "lavfi", "-i", "anullsrc=r=22050:cl=mono", "-t", "3600", "-c:a", "aac", "-b:a", "12k", str(silence_m4a))

for a in catalog["albums"]:
    folder = music / safe(a["artist"]) / safe(f"{a['title']} ({a['year']})"); folder.mkdir(parents=True, exist_ok=True)
    cover(covers / f"{a['key']}.jpg", folder / "cover.jpg")
    for n, t in enumerate(a["tracks"], 1):
        out = folder / f"{n:02d} - {safe(t['title'])}.mp3"
        if t.get("lyrics"):
            lrc = "".join(f"[{int(at // 60):02d}:{at % 60:05.2f}]{text}\n" for at, text in t["lyrics"])
            out.with_suffix(".lrc").write_text(f"[ti:{t['title']}]\n[ar:{a['artist']}]\n" + lrc)
        if out.exists():
            continue
        run("-stream_loop", "-1", "-i", str(silence_mp3), "-t", str(t["seconds"]), "-c", "copy", "-id3v2_version", "3",
            "-metadata", f"title={t['title']}", "-metadata", f"artist={a['artist']}", "-metadata", f"album_artist={a['artist']}",
            "-metadata", f"album={a['title']}", "-metadata", f"track={n}/{len(a['tracks'])}", "-metadata", f"date={a['year']}",
            "-metadata", f"genre={a['genre']}", str(out))
    print("album", a["artist"], "-", a["title"])

for b in catalog["books"]:
    folder = books / safe(b["author"]) / safe(b["title"]); folder.mkdir(parents=True, exist_ok=True)
    cover(covers / f"{b['key']}.jpg", folder / "cover.jpg")
    out = folder / f"{safe(b['title'])}.m4b"
    if out.exists():
        continue
    total = int(b["hours"] * 3600 * 1000); step = total // b["chapters"]
    meta = tmp / "chapters.txt"
    lines = [";FFMETADATA1", f"title={b['title']}", f"artist={b['author']}", f"album_artist={b['author']}", f"album={b['title']}",
             f"genre={b['genre']}", f"composer={b['narrator']}", f"date={b['year']}"]
    for i in range(b["chapters"]):
        end = total if i == b["chapters"] - 1 else (i + 1) * step
        lines += ["[CHAPTER]", "TIMEBASE=1/1000", f"START={i * step}", f"END={end}", f"title=Chapter {i + 1}"]
    meta.write_text("\n".join(lines) + "\n")
    run("-stream_loop", "-1", "-i", str(silence_m4a), "-i", str(meta), "-map", "0:a", "-map_metadata", "1", "-map_chapters", "1",
        "-t", f"{total / 1000:.3f}", "-c", "copy", "-f", "mp4", str(out))
    print("book", b["title"])
