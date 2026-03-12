import sqlite3
import os

# Updated to the correct path found in workspace/runtime/finally-a-value-bot.db
db_path = "workspace/runtime/finally-a-value-bot.db"

if not os.path.exists(db_path):
    print(f"Error: Database not found at {db_path}")
    print("Checking for absolute path...")
    abs_db_path = "/home/ken/big_storage/projects/finally-a-value-bot/workspace/runtime/finally-a-value-bot.db"
    if os.path.exists(abs_db_path):
        db_path = abs_db_path
    else:
        print(f"Error: Database also not found at {abs_db_path}")
        exit(1)

conn = sqlite3.connect(db_path)
cursor = conn.cursor()

chat_id = 997894126
# The prompt for the agent to run the indexing
prompt = "Run the vault indexing script: /app/workspace/shared/test-venv/bin/python3 /app/workspace/skills/index-vault/index_vault.py. When finished, send a message to the user confirming the indexing status."
schedule_type = "cron"
schedule_value = "0 0 */6 * * *" # Every 6 hours

try:
    cursor.execute("""
        INSERT INTO scheduled_tasks (chat_id, prompt, schedule_type, schedule_value, status, created_at)
        VALUES (?, ?, ?, ?, 'active', datetime('now'))
    """, (chat_id, prompt, schedule_type, schedule_value))
    conn.commit()
    print(f"Successfully scheduled vault indexing for chat {chat_id} using database at {db_path}")
except Exception as e:
    print(f"Error: {e}")
finally:
    conn.close()
