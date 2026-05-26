---
name: session-db-migration
description: Writing schema migrations for SQLite FTS5 session database
triggers:
  - "FTS5 migration"
  - "schema version"
  - "session_db migration"
  - "backfill"
---

# Session DB FTS5 Migration

## When to add a migration

1. Schema changes (new columns, tables, indexes)
2. FTS5 content formula changes (changing what gets indexed)
3. Trigger changes (after insert/update/delete logic)

## How to add a migration

In `src/extras/session_db.rs`:

1. Bump `SCHEMA_VERSION`
2. Add `run_migration_vN()` method
3. Add `if current < N { self.run_migration_vN()?; }` in `migrate()`
4. Update `pragma user_version` at end of `migrate()` to use `SCHEMA_VERSION`

## CRITICAL: FTS5 formula changes

External-content FTS5 tables (`content=messages, content_rowid=id`) use triggers to populate the index. When the trigger formula changes (e.g. adding `tool_name` to the indexed content), `INSERT INTO messages_fts(messages_fts) VALUES('rebuild')` does NOT work — it re-indexes using the OLD formula.

**Correct approach:**
```sql
-- 1. Drop old triggers, create new ones with new formula
DROP TRIGGER IF EXISTS messages_ai;
CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, content) VALUES (
        new.id,
        COALESCE(new.content, '') || ' ' ||
        COALESCE(new.tool_name, '') || ' ' ||
        COALESCE(new.tool_calls, '')
    );
END;
-- (repeat for messages_ad, messages_au)

-- 2. Clear old index
DELETE FROM messages_fts;

-- 3. Re-insert with new formula
INSERT INTO messages_fts(rowid, content)
SELECT id,
       COALESCE(content, '') || ' ' ||
       COALESCE(tool_name, '') || ' ' ||
       COALESCE(tool_calls, '')
FROM messages;
```

## Testing migrations

Write a test that:
1. Creates a DB file manually (not through SessionDb) with the old schema
2. Inserts sample data with the old trigger format
3. Sets `user_version` to old version
4. Opens via `SessionDb::open()` — triggers migration
5. Asserts that data is now queryable with the new formula

Use `Connection::open_with_flags` + `execute_batch` for manual setup, then `conn.close()` before re-opening through `SessionDb`.

## Pitfalls

- `INSERT INTO messages_fts(messages_fts) VALUES ('rebuild')` uses the trigger formula that existed at rebuild time. If you already replaced the trigger, it uses the new formula — but if there's existing data, it wasn't indexed by the new formula unless the trigger was dropped first. SAFER to always DELETE + INSERT SELECT.
- `execute_batch` can't mix `CREATE VIRTUAL TABLE` with normal SQL reliably. Create FTS5 tables in separate `execute` calls.
- Temp directories from `temp_db()` helper should use process ID + counter to avoid collisions in parallel tests.
