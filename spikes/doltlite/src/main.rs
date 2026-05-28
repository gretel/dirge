//! doltlite FFI spike — prove the SQLite-compatible C API works
//! against libdoltlite and that the Git/SQL primitives
//! (`dolt_commit`, `dolt_log`, `dolt_at_*`, `dolt_diff_*`) are
//! reachable from raw FFI without bindgen scaffolding.
//!
//! Run with `cargo run --bin doltlite-spike`.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr;

// ── Minimal SQLite C ABI (extern "C") — handful of calls we need ──

#[allow(non_camel_case_types)]
type sqlite3 = c_void;
#[allow(non_camel_case_types)]
type sqlite3_stmt = c_void;

const SQLITE_OK: c_int = 0;
const SQLITE_ROW: c_int = 100;
const SQLITE_DONE: c_int = 101;

unsafe extern "C" {
    fn sqlite3_open(filename: *const c_char, db: *mut *mut sqlite3) -> c_int;
    fn sqlite3_close(db: *mut sqlite3) -> c_int;
    fn sqlite3_exec(
        db: *mut sqlite3,
        sql: *const c_char,
        callback: Option<
            unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int,
        >,
        ctx: *mut c_void,
        errmsg: *mut *mut c_char,
    ) -> c_int;
    fn sqlite3_prepare_v2(
        db: *mut sqlite3,
        sql: *const c_char,
        n_byte: c_int,
        stmt: *mut *mut sqlite3_stmt,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_step(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_finalize(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_column_text(stmt: *mut sqlite3_stmt, col: c_int) -> *const c_char;
    fn sqlite3_column_int(stmt: *mut sqlite3_stmt, col: c_int) -> c_int;
    fn sqlite3_errmsg(db: *mut sqlite3) -> *const c_char;
    fn sqlite3_libversion() -> *const c_char;
}

// ── Helpers ────────────────────────────────────────────────────

struct DoltDb(*mut sqlite3);

impl DoltDb {
    fn open(path: &str) -> Result<Self, String> {
        let c_path = CString::new(path).unwrap();
        let mut db: *mut sqlite3 = ptr::null_mut();
        let rc = unsafe { sqlite3_open(c_path.as_ptr(), &mut db) };
        if rc != SQLITE_OK {
            return Err(format!("sqlite3_open rc={}", rc));
        }
        Ok(DoltDb(db))
    }

    fn exec(&self, sql: &str) -> Result<(), String> {
        let c_sql = CString::new(sql).unwrap();
        let mut errmsg: *mut c_char = ptr::null_mut();
        let rc = unsafe {
            sqlite3_exec(
                self.0,
                c_sql.as_ptr(),
                None,
                ptr::null_mut(),
                &mut errmsg,
            )
        };
        if rc != SQLITE_OK {
            let msg = if errmsg.is_null() {
                "(no message)".to_string()
            } else {
                unsafe { CStr::from_ptr(errmsg).to_string_lossy().into_owned() }
            };
            return Err(format!("sqlite3_exec rc={} {}: {}", rc, sql, msg));
        }
        Ok(())
    }

    /// Run a query that returns rows of (text, text) and print them.
    fn dump(&self, sql: &str) -> Result<(), String> {
        let c_sql = CString::new(sql).unwrap();
        let mut stmt: *mut sqlite3_stmt = ptr::null_mut();
        let rc = unsafe {
            sqlite3_prepare_v2(self.0, c_sql.as_ptr(), -1, &mut stmt, ptr::null_mut())
        };
        if rc != SQLITE_OK {
            let msg = unsafe { CStr::from_ptr(sqlite3_errmsg(self.0)) }
                .to_string_lossy()
                .into_owned();
            return Err(format!("prepare rc={} {}: {}", rc, sql, msg));
        }
        println!("  query: {}", sql);
        loop {
            let step_rc = unsafe { sqlite3_step(stmt) };
            if step_rc == SQLITE_DONE {
                break;
            }
            if step_rc != SQLITE_ROW {
                unsafe { sqlite3_finalize(stmt) };
                return Err(format!("step rc={}", step_rc));
            }
            let mut row = String::new();
            for col in 0..8 {
                let text_ptr = unsafe { sqlite3_column_text(stmt, col) };
                if text_ptr.is_null() {
                    break;
                }
                let text = unsafe { CStr::from_ptr(text_ptr as *const c_char) }
                    .to_string_lossy();
                if col > 0 {
                    row.push_str("  |  ");
                }
                row.push_str(&text);
            }
            println!("    → {}", row);
        }
        unsafe { sqlite3_finalize(stmt) };
        Ok(())
    }

    fn scalar_int(&self, sql: &str) -> Result<i64, String> {
        let c_sql = CString::new(sql).unwrap();
        let mut stmt: *mut sqlite3_stmt = ptr::null_mut();
        let rc = unsafe {
            sqlite3_prepare_v2(self.0, c_sql.as_ptr(), -1, &mut stmt, ptr::null_mut())
        };
        if rc != SQLITE_OK {
            return Err(format!("prepare rc={}: {}", rc, sql));
        }
        let mut out: i64 = 0;
        if unsafe { sqlite3_step(stmt) } == SQLITE_ROW {
            out = unsafe { sqlite3_column_int(stmt, 0) } as i64;
        }
        unsafe { sqlite3_finalize(stmt) };
        Ok(out)
    }
}

impl Drop for DoltDb {
    fn drop(&mut self) {
        unsafe { sqlite3_close(self.0) };
    }
}

fn print_header(title: &str) {
    println!("\n── {} ──", title);
}

fn main() {
    let version = unsafe {
        CStr::from_ptr(sqlite3_libversion())
            .to_string_lossy()
            .into_owned()
    };
    println!("Linked SQLite-API version: {}", version);

    // dirge-style on-disk DB. Use a tempdir so reruns are clean.
    let tmpdir = std::env::temp_dir().join(format!(
        "doltlite-spike-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmpdir);
    std::fs::create_dir_all(&tmpdir).unwrap();
    let db_path = tmpdir.join("dirge.db");
    println!("DB path: {}", db_path.display());

    let db = DoltDb::open(db_path.to_str().unwrap()).expect("open");

    // ── Test 1: basic SQL + schema (mirrors dirge SessionDb shape) ──
    print_header("Test 1 — schema + insert + select");
    db.exec(
        "CREATE TABLE sessions (
            id TEXT PRIMARY KEY,
            source TEXT,
            created_at TEXT
        )",
    )
    .expect("create sessions");
    db.exec(
        "CREATE TABLE messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT,
            role TEXT,
            content TEXT,
            created_at TEXT
        )",
    )
    .expect("create messages");
    db.exec("INSERT INTO sessions VALUES ('sess-1', 'cli', '2026-05-28T00:00:00Z')")
        .expect("insert session");
    db.exec(
        "INSERT INTO messages (session_id, role, content, created_at)
         VALUES
            ('sess-1', 'user', 'how do I use FTS5 in dirge?', '2026-05-28T00:00:01Z'),
            ('sess-1', 'assistant', 'dirge uses sqlite3 fts5 virtual tables', '2026-05-28T00:00:02Z'),
            ('sess-1', 'user', 'what about trigram fallback for CJK?', '2026-05-28T00:00:03Z')",
    )
    .expect("insert messages");

    let count = db.scalar_int("SELECT COUNT(*) FROM messages").unwrap();
    println!("  messages count: {} (expected 3)", count);
    assert_eq!(count, 3);

    // ── Test 2: FTS5 virtual table (dirge's session_search backbone) ──
    print_header("Test 2 — FTS5 virtual table");
    match db.exec(
        "CREATE VIRTUAL TABLE messages_fts USING fts5(content, session_id UNINDEXED)",
    ) {
        Ok(_) => println!("  FTS5 virtual table created ✓"),
        Err(e) => println!("  FTS5 virtual table FAILED: {}", e),
    }
    db.exec("INSERT INTO messages_fts(content, session_id) SELECT content, session_id FROM messages")
        .expect("populate fts");
    println!("  FTS5 query for 'FTS5':");
    db.dump("SELECT session_id, snippet(messages_fts, 0, '<', '>', '…', 8) FROM messages_fts WHERE messages_fts MATCH 'FTS5'")
        .expect("fts5 query");
    println!("  FTS5 prefix query 'trig*':");
    db.dump("SELECT session_id, content FROM messages_fts WHERE messages_fts MATCH 'trig*'")
        .expect("fts5 prefix");

    // ── Test 3: doltlite Git primitives via SQL ──
    print_header("Test 3 — doltlite Git primitives (dolt_log / dolt_commit / dolt_at)");
    println!("  Initial dolt_log() (should show schema-init commit):");
    match db.dump("SELECT * FROM dolt_log()") {
        Ok(_) => println!("  dolt_log() ✓"),
        Err(e) => println!("  dolt_log() FAILED: {}", e),
    }

    println!("  Calling dolt_add('-A') + dolt_commit():");
    match db.dump("SELECT dolt_add('-A')") {
        Ok(_) => {}
        Err(e) => println!("  dolt_add FAILED: {}", e),
    }
    match db.dump("SELECT dolt_commit('-m', 'initial seed')") {
        Ok(_) => println!("  dolt_commit ✓"),
        Err(e) => println!("  dolt_commit FAILED: {}", e),
    }

    println!("  After commit, append another message:");
    db.exec(
        "INSERT INTO messages (session_id, role, content, created_at)
         VALUES ('sess-1', 'assistant', 'trigram tokenizer is built-in', '2026-05-28T00:00:04Z')",
    )
    .expect("insert post-commit");
    let count_after = db.scalar_int("SELECT COUNT(*) FROM messages").unwrap();
    println!("  messages count now: {} (expected 4)", count_after);

    println!("  dolt_diff_messages between HEAD~1 (initial) and working set:");
    match db.dump("SELECT from_role, to_role, from_content, to_content, diff_type FROM dolt_diff_messages WHERE from_commit = 'HEAD' AND to_commit = 'WORKING'") {
        Ok(_) => println!("  dolt_diff_messages ✓"),
        Err(e) => println!("  dolt_diff_messages FAILED: {}", e),
    }

    println!("  dolt_history_messages (per-row history):");
    match db.dump("SELECT id, role, content, commit_hash FROM dolt_history_messages LIMIT 5") {
        Ok(_) => println!("  dolt_history_messages ✓"),
        Err(e) => println!("  dolt_history_messages FAILED: {}", e),
    }

    // ── Test 4: branching (per-session-fork memory state) ──
    print_header("Test 4 — branching for per-session forks");
    match db.dump("SELECT dolt_branch('feature-fork')") {
        Ok(_) => println!("  dolt_branch ✓"),
        Err(e) => println!("  dolt_branch FAILED: {}", e),
    }
    match db.dump("SELECT * FROM dolt_branches") {
        Ok(_) => println!("  dolt_branches ✓"),
        Err(e) => println!("  dolt_branches FAILED: {}", e),
    }

    // ── Test 5: clean shutdown ──
    print_header("Test 5 — clean shutdown");
    drop(db);
    println!("  DB closed cleanly ✓");

    let on_disk_size: u64 = walkdir(&tmpdir);
    println!("  On-disk footprint: {} bytes", on_disk_size);

    println!("\n=== SPIKE COMPLETE ===");
}

fn walkdir(p: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(p) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += walkdir(&path);
            } else if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}
