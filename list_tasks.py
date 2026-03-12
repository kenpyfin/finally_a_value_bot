import sqlite3
import os

db_path = "/home/ken/big_storage/projects/finally-a-value-bot/workspace/runtime/db/finally-a-value-bot.db"

if not os.path.exists(db_path):
    print(f"Error: Database not found at {db_path}")
    exit(1)

conn = sqlite3.connect(db_path)
cursor = conn.cursor()

try:
    cursor.execute("SELECT id, chat_id, prompt, schedule_type, schedule_value FROM scheduled_tasks")
    rows = cursor.fetchall()
    print("ID | Chat ID | Prompt | Type | Value")
    print("-" * 50)
    for row in rows:
        print(f"{row[0]} | {row[1]} | {row[2][:50]}... | {row[3]} | {row[4]}")
except Exception as e:
    print(f"Error: {e}")
finally:
    conn.close()
