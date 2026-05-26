---
name: flaky-test-env-var-fix
description: Fix flaky tests caused by concurrent env variable mutations
triggers:
  - "flaky test env var"
  - "env::set_var race"
  - "EnvGuard"
  - "DIRGE_PROJECT_ROOT test"
---

# Fix Flaky Tests from Env Var Mutations

## Problem

`std::env::set_var` and `std::env::remove_var` mutate global process state. When multiple tests run in parallel and mutate the same env var, they race — one test's assertion sees another test's value.

Rust marks these functions `unsafe` for this reason. `cargo test` runs tests in parallel by default.

## Fix Pattern

```rust
#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    /// Serializes env-mutating tests. Only one holds this at a time.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// RAII guard: calls set_var on construction, remove_var on Drop.
    /// Panic-safe — Drop always runs (including during unwinding).
    struct EnvGuard;

    impl EnvGuard {
        fn set(key: &str, value: &str) -> Self {
            unsafe { std::env::set_var(key, value) };
            EnvGuard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe { std::env::remove_var("DIRGE_PROJECT_ROOT") };
        }
    }

    #[test]
    fn env_mutating_test() {
        let _lock = ENV_LOCK.lock().unwrap();    // serialize
        let _guard = EnvGuard::set("KEY", "val"); // set + auto-clear
        // ... assertions that depend on KEY=val ...
    }
}

## Why this works

1. Mutex serializes: only one env-mutating test runs at any time
2. RAII guard: env var is always cleared when the test exits, even on panic
3. Other tests never see the overridden value

## Where applied

- `src/extras/dirge_paths.rs`: `env_override_wins_over_git_detection`, `env_override_ignores_missing_directory`
