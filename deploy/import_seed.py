#!/usr/bin/env python3
"""Import scripts/seed_data.json into a running SurrealDB v3 server.

Deployment copy of scripts/import_seed.py (which stays untracked scraper
tooling): the import routine is byte-identical, the entry point gains the
subcommands first-time setup needs:

  import            wipe and recreate every seeded table from the fixture
                    (the original behaviour; wipes user/build/review too)
  --if-empty        skip the import when the cookie table already has rows
  --ensure-indexes  create the indexes the app assumes (unique username)
  --create-admin U P  create (or refuse to duplicate) an admin user, hashing
                    the password server-side with crypto::argon2::generate,
                    the same primitive src/session.rs uses

Connection settings come from the environment, like the original:

  IMPORT_URL  (default ws://127.0.0.1:8100)
  IMPORT_NS   (default cookierun)
  IMPORT_DB   (default cookierun)
  IMPORT_USER (default root)
  IMPORT_PASS (default root)
"""

import json
import os
import sys
import time
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FIXTURE = os.path.join(ROOT, 'scripts', 'seed_data.json')

URL = os.environ.get('IMPORT_URL', 'ws://127.0.0.1:8100')
NS = os.environ.get('IMPORT_NS', 'cookierun')
DB = os.environ.get('IMPORT_DB', 'cookierun')
USER = os.environ.get('IMPORT_USER', 'root')
PASS = os.environ.get('IMPORT_PASS', 'root')

# grade value -> grade::rank (see src/grade.rs); E (0) ranks above L (6)
RANK = {1: 0, 2: 1, 3: 2, 4: 3, 5: 4, 6: 5, 0: 6}
GRADED = {'cookie', 'pet', 'treasure', 'ingredient', 'skin'}

# entity -> (translation table, owner column, fields the app reads back)
ENTITY = {
    'cookie': ('cookie_translation', 'cookie_id',
               ['name', 'abilities', 'description', 'power_plus',
                'power_plus_requirement', 'unlock_goal']),
    'pet': ('pet_translation', 'pet_id',
            ['name', 'abilities', 'description']),
    'treasure': ('treasure_translation', 'treasure_id', ['name', 'description']),
    'relic': ('relic_translation', 'relic_id', ['name', 'description']),
    'episode': ('episode_translation', 'episode_id', ['name', 'description']),
    'ingredient': ('ingredient_translation', 'ingredient_id', ['name', 'description']),
    'jelly': ('jelly_translation', 'jelly_id', ['name', 'description']),
    'skin': ('skin_translation', 'skin_id', ['name', 'description']),
}

# tables imported verbatim (support data nothing reads yet, kept for parity)
RAW = ['quest', 'episode_stage', 'episode_relic', 'episode_draw_reward',
       'episode_box_odds', 'ingredient_recipe', 'economy_table', 'economy_row',
       'jelly_maker']

def to_unix(stamp):
    """ISO 8601 Z timestamp -> unix seconds; already-numeric passes through.
    The 1970 sentinel (no known date) stays 0."""
    if not stamp:
        return 0
    if isinstance(stamp, (int, float)):
        return int(stamp)
    t = stamp.replace('Z', '+00:00')
    from datetime import datetime, timezone
    dt = datetime.fromisoformat(t)
    secs = int(dt.astimezone(timezone.utc).timestamp())
    return 0 if secs <= 0 else secs

def rpc(query, vars_, ns=NS, db=DB):
    """One JSON-RPC query call over the HTTP endpoint."""
    import base64
    body = json.dumps({
        'id': str(time.time_ns()),
        'method': 'query',
        'params': (query, vars_),
    }).encode()
    url = URL.replace('ws://', 'http://').replace('wss://', 'https://').rstrip('/') + '/rpc'
    headers = {'Content-Type': 'application/json'}
    if ns:
        headers['Surreal-NS'] = ns
    if db:
        headers['Surreal-DB'] = db
    req = urllib.request.Request(url, data=body, headers=headers)
    token = base64.b64encode(f'{USER}:{PASS}'.encode()).decode()
    req.add_header('Authorization', 'Basic ' + token)
    with urllib.request.urlopen(req, timeout=300) as res:
        out = json.loads(res.read().decode())
    if isinstance(out, dict) and out.get('error'):
        raise RuntimeError(out['error'])
    if not isinstance(out, dict):
        raise RuntimeError(f'unexpected rpc response: {str(out)[:200]}')
    for part in out.get('result', []):
        if isinstance(part, dict) and part.get('status') == 'ERR':
            raise RuntimeError(part.get('detail') or part.get('result'))
    return out

def q(query, vars_=None):
    """First query result as a list of records."""
    out = rpc(query, vars_ or {})
    parts = out.get('result') or []
    if not parts:
        return []
    return parts[0].get('result') or []

def table_count(name):
    """Row count; a not-yet-imported table counts as 0 (v3 errors on
    SELECT against a missing table instead of returning an empty set, and
    count() without GROUP ALL returns one row per record)."""
    try:
        n = q(f'SELECT count() AS n FROM {name} GROUP ALL') or [{'n': 0}]
    except RuntimeError as exc:
        if 'does not exist' in str(exc):
            return 0
        raise
    return n[0].get('n') or 0

def write_table(table, rows, schema=None):
    """Wipes then recreates one table in chunks of bound CREATE statements.
    `rows` is a list of (record_id, [(field, value), ...]); None values are
    dropped so absent record fields stay absent."""
    if not rows:
        return
    print(f'{table}: {len(rows)} rows', flush=True)
    if schema:
        rpc(f'REMOVE TABLE IF EXISTS {table}; DEFINE TABLE {table} {schema}', {})
    for i in range(0, len(rows), 400):
        chunk = rows[i:i + 400]
        if i == 0:
            if table_count(table):
                statements = [f'DELETE {table}']
            else:
                # fresh database: DELETE on a missing table errors in v3,
                # so define it instead (the original dev flow only ever
                # wiped tables that already existed)
                statements = [f'DEFINE TABLE IF NOT EXISTS {table} SCHEMALESS']
        else:
            statements = []
        vars_ = {}
        for j, (rid, fields) in enumerate(chunk):
            present = [(k, v) for k, v in fields if v is not None]
            for key, val in present:
                vars_[f'v{j}_{key}'] = val
            sets = ', '.join(f'{key} = $v{j}_{key}' for key, _ in present)
            if not sets:
                continue
            statements.append(f'CREATE {table}:{rid} SET {sets}')
        rpc(';\n'.join(statements), vars_)

def ensure_scope():
    """SurrealDB v3 does not auto-create namespaces for raw queries (the
    SDK's connect does, which is why the app itself works on an empty
    server). Our rpc calls run against a namespace/database that must exist,
    so create both first — root scope for the namespace, then the namespace
    scope for the database."""
    rpc('DEFINE NAMESPACE IF NOT EXISTS type::string($ns)', {'ns': NS},
        ns=None, db=None)
    rpc('DEFINE DATABASE IF NOT EXISTS type::string($db)', {'db': DB},
        ns=NS, db=None)

def do_import(fixture_path=None):
    with open(fixture_path or FIXTURE, encoding='utf-8') as fh:
        data = json.load(fh)

    # --- translations nest per language -----------------------------------
    trs = {}  # (entity, owner_id) -> {lang: {field: value}}
    for entity, (ttable, ocol, fields) in ENTITY.items():
        owner_col = 'owner_id' if entity == 'cookie' else f'{entity}_id'
        for row in data.get(ttable, []):
            trs.setdefault((entity, row[owner_col]), {})[row['lang']] = {
                f: row.get(f, '') or '' for f in fields
            }

    # --- treasure effect lines stitch --------------------------------------
    # text per (effect_id, lang), membership per (treasure, effect, state),
    # values per (treasure, effect, state, level)
    etext = {}
    for row in data.get('effect_translation', []):
        etext[(row['effect_id'], row['lang'])] = (row.get('description') or row.get('name') or '')
    membership = {}
    for row in data.get('treasure_effect', []):
        state = 1 if row.get('state') == 'blessed' else 0
        membership.setdefault((row['treasure_id'], row['effect_id'], state), None)
    levels = {}
    for row in data.get('treasure_level', []):
        state = 1 if row.get('state') == 'blessed' else 0
        key = (row['treasure_id'], row['effect_id'], state)
        levels.setdefault(key, {})[row['level']] = row.get('values') or ''

    lines = {}  # treasure_id -> [line]
    for (tid, eid, state) in sorted(membership):
        vals = levels.get((tid, eid, state), {})
        # shape must match EffectLineRow in src/db.rs: state is int 0/1 and
        # the 0-9 ladder is strings
        lines.setdefault(tid, []).append({
            'state': state,
            'en': etext.get((eid, 'en'), ''),
            'th': etext.get((eid, 'th'), ''),
            'values': [vals.get(l, '') for l in range(10)],
        })

    # --- combi: effect text inline per language ----------------------------
    combi = []
    for row in data.get('combi_bonus', []):
        eid = row.get('effect_id')
        combi.append((row['id'], [
            ('cookie_id', row.get('cookie_id') or 0),
            ('pet_id', row.get('pet_id') or 0),
            ('is_hidden', bool(row.get('is_hidden'))),
            ('en', etext.get((eid, 'en'), '')),
            ('th', etext.get((eid, 'th'), '')),
        ]))

    # --- gacha pools: entries denormalized, ordered; the entry refs must
    # never carry a null member (the v3 reader rejects null where it wants
    # int | none), so absent sides are dropped and stay NONE ----------------
    entries = {}
    for row in sorted(data.get('gacha_pool_entry', []), key=lambda r: r.get('sort_order') or 0):
        entry = {}
        if row.get('treasure_id'):
            entry['t'] = row['treasure_id']
        if row.get('pet_id'):
            entry['p'] = row['pet_id']
        entry['odds'] = row.get('odds') or 0.0
        entries.setdefault(row['pool_id'], []).append(entry)

    # --- entities -----------------------------------------------------------
    for entity, (_ttable, _ocol, _fields) in ENTITY.items():
        pk = f'{entity}_id'
        rows = []
        for src in data.get(entity, []):
            fields = []
            for key, val in src.items():
                if key == pk:
                    continue
                if key == 'release_date':
                    val = to_unix(val)
                elif key in ('base_treasure_id', 'unlock_cookie_id', 'unlock_pet_id',
                             'drop_episode_id', 'episode_id', 'cookie_id', 'pet_id'):
                    val = val if val else None
                fields.append((key, val))
            if entity in GRADED:
                grade = src.get('grade') or 0
                fields.append(('rank', RANK.get(grade, -1)))
            fields.append(('tr', trs.get((entity, src[pk]), {})))
            if entity == 'treasure':
                fields.append(('effect_lines', lines.get(src[pk], [])))
            rows.append((src[pk], fields))
        write_table(entity, rows)

    write_table('combi', combi, schema='SCHEMALESS')

    pools = []
    for src in data.get('gacha_pool', []):
        pools.append((src['pool_id'], [
            ('name', src.get('name') or ''),
            ('tier', src.get('tier') or ''),
            ('entries', entries.get(src['pool_id'], [])),
        ]))
    write_table('gacha_pool', pools)

    for table in RAW:
        pk = f'{table}_id'
        rows = [(src[pk], [(k, v) for k, v in src.items() if k != pk])
                for src in data.get(table, [])]
        write_table(table, rows)

    # --- user tables the write side needs; the fixture carries no rows ------
    for table in ['build', 'review', 'user']:
        rpc(f'REMOVE TABLE IF EXISTS {table}; DEFINE TABLE {table} SCHEMALESS', {})
        print(f'{table}: empty', flush=True)

    print('done', flush=True)

def do_ensure_indexes():
    rpc('DEFINE INDEX IF NOT EXISTS user_username ON user FIELDS username UNIQUE', {})
    print('ensure-indexes: done')

def do_create_admin(username, password):
    if not username or not password:
        raise RuntimeError('admin username and password must not be empty')
    try:
        existing = q('SELECT username FROM user WHERE username = $u LIMIT 1', {'u': username})
    except RuntimeError as exc:
        if 'does not exist' in str(exc):
            existing = []  # user table not created yet (no import ran)
        else:
            raise
    if existing:
        raise RuntimeError(f'user {username!r} already exists')
    rid = table_count('user') + 1
    hash_rows = q('RETURN crypto::argon2::generate($p)', {'p': password})
    if not hash_rows:
        raise RuntimeError('crypto::argon2::generate returned nothing')
    phc = hash_rows[0]
    q('CREATE type::record("user", $id) SET username = $u, password = $phc, '
      'is_admin = true, created_at = time::unix()', {'id': rid, 'u': username, 'phc': phc})
    print(f'create-admin: {username} (user:{rid}, is_admin=true)')

def main():
    import argparse
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--fixture', default=FIXTURE,
                    help='path to seed_data.json (default: scripts/seed_data.json)')
    ap.add_argument('--if-empty', action='store_true',
                    help='only run the import when the cookie table is empty')
    ap.add_argument('--ensure-indexes', action='store_true',
                    help='create the indexes the app assumes')
    ap.add_argument('--create-admin', nargs=2, metavar=('USER', 'PASS'),
                    help='create an is_admin user (server-side argon2id hash)')
    ap.add_argument('--create-admin-if-empty', nargs=2, metavar=('USER', 'PASS'),
                    help='like --create-admin, but a no-op when user table has rows')
    args = ap.parse_args()

    ensure_scope()

    if args.if_empty:
        count = table_count('cookie')
        if count:
            print(f'cookie table already has {count} rows; import skipped')
        else:
            do_import(args.fixture)
    else:
        do_import(args.fixture)

    if args.ensure_indexes:
        do_ensure_indexes()
    if args.create_admin:
        do_create_admin(*args.create_admin)
    if args.create_admin_if_empty:
        count = table_count('user')
        if count:
            print(f'user table already has {count} rows; admin creation skipped')
        else:
            do_create_admin(*args.create_admin_if_empty)
    return 0

if __name__ == '__main__':
    sys.exit(main())