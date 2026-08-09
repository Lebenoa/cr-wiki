"""Download every treasure image referenced by the DB into static/img/treasures/.

The wiki stores images under hash paths, so each File:<name> is resolved to a
real URL via the MediaWiki imageinfo API, then downloaded to the exact
filename the DB uses (templates render /img/treasures/<image>).
"""

import json
import os
import sys
import time
import urllib.parse
import urllib.request

API = "https://cookierun.wiki/mw/api.php"
OUT = "static/img/treasures"
BATCH = 40
UA = "Mozilla/5.0 (compatible; treasure-importer/1.0)"


FANDOM_API = "https://cookierun.fandom.com/api.php"


def api_query(base, params):
    params["format"] = "json"
    params["formatversion"] = "2"
    url = base + "?" + urllib.parse.urlencode(params)
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r)


def resolve(urls_for, base):
    """Resolve File: titles -> {title: imageinfo url} via imageinfo."""
    titles = ["File:" + n for n in urls_for]
    data = api_query(base, {
        "action": "query",
        "prop": "imageinfo",
        "iiprop": "url",
        "titles": "|".join(titles),
    })
    out = {}
    for page in data["query"]["pages"]:
        if page.get("missing"):
            continue
        info = page.get("imageinfo") or []
        if info:
            out[page["title"]] = info[0].get("url", "")
    return out


def main():
    os.makedirs(OUT, exist_ok=True)
    # distinct image names from the treasure tables
    import sqlite3
    c = sqlite3.connect("sqlite.db")
    names = [r[0] for r in c.execute(
        "SELECT DISTINCT image FROM treasure WHERE image IS NOT NULL")]

    missing_on_disk = [n for n in names if not os.path.exists(os.path.join(OUT, n))]
    print(f"{len(names)} distinct images, {len(missing_on_disk)} missing on disk")

    downloaded = 0
    unresolved = []
    failed = []
    for i in range(0, len(missing_on_disk), BATCH):
        batch = missing_on_disk[i:i + BATCH]
        resolved = resolve(batch, API)
        for name in batch:
            dest = os.path.join(OUT, name)
            if os.path.exists(dest):
                continue
            # imageinfo keys are normalized titles (underscores become spaces)
            key = "File:" + name
            url = resolved.get(key) or resolved.get(key.replace('_', ' '))
            if not url:
                # the new wiki is under construction; the old Fandom wiki still
                # hosts the migrated Classic images
                fand = resolve([name], FANDOM_API)
                url = fand.get(key) or fand.get(key.replace('_', ' '))
            if not url:
                unresolved.append(name)
                continue
            try:
                req = urllib.request.Request(url, headers={"User-Agent": UA})
                with urllib.request.urlopen(req, timeout=60) as r:
                    data = r.read()
                with open(dest, "wb") as f:
                    f.write(data)
                downloaded += 1
            except Exception as e:
                failed.append((name, str(e)))
        time.sleep(0.3)

    print(f"downloaded: {downloaded}")
    if unresolved:
        print("unresolved:", len(unresolved))
        for n in unresolved[:20]:
            print("  ", n)
    if failed:
        print("failed:", len(failed))
        for n, e in failed[:10]:
            print("  ", n, "->", e)
    sys.exit(0 if not unresolved and not failed else 1)


if __name__ == "__main__":
    main()
