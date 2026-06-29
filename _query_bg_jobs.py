#!/usr/bin/env python3
import sqlite3
import os

db_paths = [
    "/home/ken/big_storage/projects/finally-a-value-bot/workspace/runtime/finally_a_value_bot.db",
    "/home/ken/big_storage/projects/finally-a-value-bot/workspace/runtime/db/finally-a-value-bot.db",
    "/home/ken/big_storage/projects/finally-a-value-bot/workspace/runtime/finally-a-value-bot.db",
]

for db_path in db_paths:
    print(f"=== DB: {db_path} exists={os.path.exists(db_path)} ===")
    if not os.path.exists(db_path):
        continue
    conn = sqlite3.connect(db_path)
    cur = conn.cursor()
    try:
        cur.execute(
            "SELECT id, status, command, job_type, created_at, updated_at "
            "FROM background_jobs WHERE status IN ('running','pending') "
            "AND command LIKE '%pz_v8%'"
        )
        rows = cur.fetchall()
        if rows:
            for r in rows:
                print(r)
        else:
            print("(no matching rows)")
    except Exception as e:
        print(f"ERROR: {e}")
    finally:
        conn.close()
