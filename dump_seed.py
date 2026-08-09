"""Dump the roster tables into scripts/seed_data.json (the committed seed fixture).

Matches the shape database.SeedFixture expects: table-named arrays of plain
objects with model field names. Dates are RFC3339 UTC; unknown dates (unix 0)
become the 1970 epoch sentinel that json2 round-trips to time.Time{}.
"""

import datetime
import json
import sqlite3

DB = "sqlite.db"
OUT = "scripts/seed_data.json"

EPOCH = "1970-01-01T00:00:00Z"
UNITS = {0: "flat", 1: "percent", 2: "second"}


def date_str(ts):
    if not ts:
        return EPOCH
    return datetime.datetime.fromtimestamp(ts, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def rows(c, table, cols):
    return [dict(zip(cols, r)) for r in c.execute(f"SELECT {', '.join(cols)} FROM {table} ORDER BY 1")]


def main():
    c = sqlite3.connect(DB)
    data = {
        "cookie": rows(c, "cookie", ["cookie_id", "grade", "image", "release_date"]),
        "cookie_translation": rows(c, "cookie_translation",
                                   ["cookie_translation_id", "owner_id", "lang", "name",
                                    "abilities", "description", "power_plus",
                                    "power_plus_requirement", "unlock_goal"]),
        "pet": rows(c, "pet", ["pet_id", "image", "grade", "release_date"]),
        "pet_translation": rows(c, "pet_translation",
                                ["pet_translation_id", "pet_id", "lang", "name",
                                 "abilities", "description"]),
        "treasure": rows(c, "treasure", ["treasure_id", "image", "is_evolved",
                                         "is_blessed", "release_date"]),
        "treasure_translation": rows(c, "treasure_translation",
                                     ["treasure_translation_id", "treasure_id", "lang",
                                      "name", "description"]),
        "effect": rows(c, "effect", ["effect_id"]),
        "effect_translation": rows(c, "effect_translation",
                                   ["effect_translation_id", "effect_id", "lang",
                                    "name", "description"]),
        "treasure_effect": rows(c, "treasure_effect",
                                ["treasure_effect_id", "treasure_id", "effect_id",
                                 "value", "unit"]),
    }
    # normalize dates, booleans, and enum ints
    for r in data["cookie"] + data["pet"] + data["treasure"]:
        r["release_date"] = date_str(r["release_date"])
    for r in data["treasure"]:
        r["is_evolved"] = bool(r["is_evolved"])
        r["is_blessed"] = bool(r["is_blessed"])
    for r in data["treasure_effect"]:
        r["unit"] = UNITS[r["unit"]]

    text = json.dumps(data, indent=1, ensure_ascii=False)
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(text + "\n")

    print("dumped:")
    for k, v in data.items():
        print(f"  {k}: {len(v)}")


if __name__ == "__main__":
    main()
