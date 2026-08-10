"""Backfill treasure grades from the wiki's infobox Grade field.

Grade strings map to the models.Grade enum order (e=0, c=1, b=2, a=3, s=4,
s_plus=5, l=6). Treasures whose page lacks a Grade field stay NULL, so they
render without a badge. Updates in place (ids stay stable).
"""

import json
import re
import sqlite3
import time
import urllib.parse
import urllib.request

API = "https://cookierun.wiki/mw/api.php"
GRADE_INT = {"e": 0, "c": 1, "b": 2, "a": 3, "s": 4, "s_plus": 5, "l": 6}


def api(params):
    params.update({"format": "json", "formatversion": "2"})
    url = API + "?" + urllib.parse.urlencode(params)
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r)


def parse_grade(wt):
    m = re.search(r"\|grade\s*=\s*([^|\n]+)", wt, re.I)
    if not m:
        return None
    g = re.sub(r"<!--.*?-->", "", m.group(1), flags=re.S).strip().lower()
    g = g.replace("+", "_plus").replace(" ", "_")
    return GRADE_INT.get(g)


def main():
    c = sqlite3.connect("sqlite.db")
    cols = [r[1] for r in c.execute("PRAGMA table_info(treasure)")]
    if "grade" not in cols:
        c.execute("ALTER TABLE treasure ADD COLUMN grade INTEGER")
    titles = [r[0] for r in c.execute(
        "SELECT DISTINCT name FROM treasure_translation")]
    seen = {}
    for i in range(0, len(titles), 20):
        batch = titles[i:i + 20]
        d = api({"action": "query", "prop": "revisions", "rvprop": "content",
                 "rvslots": "main", "titles": "|".join(batch)})
        for p in d["query"]["pages"]:
            wt = p.get("revisions", [{}])[0].get("slots", {}).get("main", {}).get("content", "")
            seen[p["title"]] = parse_grade(wt)
        time.sleep(0.2)

    updated = 0
    unset = 0
    for title, grade in seen.items():
        if grade is None:
            unset += 1
            continue
        cur = c.execute(
            "SELECT COUNT(*) FROM treasure t JOIN treasure_translation tt "
            "ON tt.treasure_id = t.treasure_id WHERE tt.name = ?", (title,)).fetchone()[0]
        if cur:
            c.execute(
                "UPDATE treasure SET grade = ? WHERE treasure_id IN "
                "(SELECT treasure_id FROM treasure_translation WHERE name = ?)",
                (grade, title))
            updated += cur
    c.commit()
    print(f"pages: {len(seen)}, grade rows updated: {updated}, ungraded titles: {unset}")
    graded = c.execute("SELECT COUNT(*) FROM treasure WHERE grade IS NOT NULL").fetchone()[0]
    total = c.execute("SELECT COUNT(*) FROM treasure").fetchone()[0]
    print(f"treasure rows: {graded}/{total} graded")


if __name__ == "__main__":
    main()
