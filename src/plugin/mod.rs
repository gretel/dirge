#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_manager_new() {
        let mgr = PluginManager::new();
        assert!(mgr.hooks.is_empty());
    }

    #[test]
    fn test_register_hook() {
        let mut mgr = PluginManager::new();
        mgr.register("on-init", "test-init");
        assert_eq!(mgr.hooks.len(), 1);
        assert!(mgr.hooks.contains_key("on-init"));
    }

    #[test]
    fn test_register_multiple_hooks() {
        let mut mgr = PluginManager::new();
        mgr.register("on-init", "test-init");
        mgr.register("on-prompt", "test-prompt");
        mgr.register("on-response", "test-response");
        assert_eq!(mgr.hooks.len(), 3);
    }

    #[test]
    fn test_load_and_eval_janet() {
        let mut mgr = PluginManager::new();
        let result = mgr.eval("(+ 1 2)");
        assert_eq!(result, Ok("3".to_string()));
    }

    #[test]
    fn test_load_and_eval_janet_error() {
        let mut mgr = PluginManager::new();
        let result = mgr.eval("(undefined-fn 1)");
        assert!(result.is_err());
    }

    #[test]
    fn test_dispatch_hook() {
        let mut mgr = PluginManager::new();
        mgr.eval("(defn on-init [ctx] (string \"loaded with model: \" (ctx :model)))")
            .unwrap();
        mgr.register("on-init", "on-init");
        let result = mgr.dispatch("on-init", "@{:model \"gpt-4\"}");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("loaded with model: gpt-4"));
    }

    #[test]
    fn test_harness_log() {
        let mut mgr = PluginManager::new();
        let result = mgr.eval("(harness/log \"hello from plugin\")");
        assert!(result.is_ok());
    }

    #[test]
    fn test_load_file() {
        let mut mgr = PluginManager::new();
        let fixtures = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("plugins")
            .join("test_plugin.janet");
        mgr.load_file(&fixtures).unwrap();
        mgr.register("on-init", "on-init");
        let result = mgr.dispatch("on-init", "@{:model \"test\"}");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("loaded with test"));
    }

    #[test]
    fn test_auto_discover_hooks() {
        let mut mgr = PluginManager::new();
        let fixtures = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("plugins")
            .join("test_plugin.janet");
        mgr.load_file(&fixtures).unwrap();

        // Simulate auto-discovery: check each hook and register if found.
        let hook_names = [
            "on-init", "on-prompt", "on-response",
            "on-tool-start", "on-tool-end", "on-error", "on-complete",
        ];
        let mut found = 0;
        for hook in &hook_names {
            if mgr.eval(hook).is_ok() {
                mgr.register(hook, hook);
                found += 1;
            }
        }
        assert_eq!(found, 3, "should find on-init, on-prompt, on-response");

        // on-init
        let result = mgr.dispatch("on-init", "@{:model \"test\"}");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("loaded with test"));

        // on-prompt with matching text
        let result = mgr.dispatch("on-prompt", "@{:prompt \"hello world\"}");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "greeting detected");

        // on-prompt with non-matching text
        let result = mgr.dispatch("on-prompt", "@{:prompt \"goodbye\"}");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");

        // on-response with matching text
        let result = mgr.dispatch("on-response", "@{:response \"error: panic\"}");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "error in response");

        // unknown hook returns empty
        let result = mgr.dispatch("on-tool-start", "@{:tool \"bash\"}");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
    }

    #[test]
    fn test_janet_escaping() {
        let mut mgr = PluginManager::new();

        // Define a test function
        mgr.eval(r#"(defn test-echo [ctx] (ctx :msg))"#).unwrap();
        mgr.register("on-prompt", "test-echo");

        // Quotes in text
        let result = mgr.dispatch("on-prompt", "@{:msg \"he said \\\"hello\\\"\"}");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "he said \"hello\"");

        // Backslashes in text
        let result = mgr.dispatch("on-prompt", "@{:msg \"path\\\\to\\\\file\"}");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "path\\to\\file");

        // Newlines in text
        let result = mgr.dispatch("on-prompt", "@{:msg \"line1\\nline2\"}");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "line1\nline2");
    }

    #[test]
    fn test_janet_phase_tracking() {
        let mut mgr = PluginManager::new();

        // Define test functions that use harness APIs
        mgr.eval(r#"
            (var test-phase :idle)
            (defn test-on-init [ctx]
              (harness/log "phase test loaded")
              nil)
            (defn test-on-prompt [ctx]
              (case test-phase
                :idle (do (set test-phase :active) "entered active")
                :active (do (set test-phase :done) "entered done")
                nil))
        "#).unwrap();

        mgr.register("on-init", "test-on-init");
        mgr.register("on-prompt", "test-on-prompt");

        // on-init should work
        let result = mgr.dispatch("on-init", "@{}");
        assert!(result.is_ok());

        // First prompt: idle -> active
        let result = mgr.dispatch("on-prompt", "@{:prompt \"any\"}");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "entered active");

        // Second prompt: active -> done
        let result = mgr.dispatch("on-prompt", "@{:prompt \"any\"}");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "entered done");

        // Third prompt: done -> nil
        let result = mgr.dispatch("on-prompt", "@{:prompt \"any\"}");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
    }
}

use std::collections::HashMap;

#[cfg(feature = "plugin")]
use janetrs::client::{Error as JanetError, JanetClient};

pub struct PluginManager {
    hooks: HashMap<String, Vec<String>>,
    #[cfg(feature = "plugin")]
    client: JanetClient,
    pub phase: String,
    pub pending_prompt: Option<String>,
    pub last_response: Option<String>,
}

impl PluginManager {
    pub fn new() -> Self {
        #[cfg(feature = "plugin")]
        let client = {
            let c = JanetClient::init_with_default_env().expect("Failed to initialize Janet VM");

            // Define harness API functions in Janet
            let _ = c.run(
                r#"
                (var harness-phase :idle)
                (var harness-pending nil)
                (var harness-response nil)

                (defn harness/log [msg] (print "[plugin] " msg))
                (defn harness/get-cwd [] (os/cwd))
                (defn harness/set-phase [p] (set harness-phase p))
                (defn harness/request-prompt [prompt]
                  (set harness-pending prompt))
                (defn harness/store-response [resp]
                  (set harness-response resp))
            "#,
            );

            c
        };

        PluginManager {
            hooks: HashMap::new(),
            #[cfg(feature = "plugin")]
            client,
            phase: String::from("idle"),
            pending_prompt: None,
            last_response: None,
        }
    }

    #[cfg(feature = "plugin")]
    pub fn load_file(&mut self, path: &std::path::Path) -> Result<(), String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read plugin: {e}"))?;
        self.eval(&content)?;
        Ok(())
    }

    #[cfg(not(feature = "plugin"))]
    pub fn load_file(&mut self, _path: &std::path::Path) -> Result<(), String> {
        Ok(())
    }

    pub fn register(&mut self, hook: &str, script: &str) {
        self.hooks
            .entry(hook.to_string())
            .or_default()
            .push(script.to_string());
    }

    #[cfg(feature = "plugin")]
    pub fn sync_phase(&mut self) {
        if let Ok(val) = self.client.run("harness-phase") {
            self.phase = val.to_string();
        }
    }

    #[cfg(not(feature = "plugin"))]
    pub fn sync_phase(&mut self) {}

    #[cfg(feature = "plugin")]
    pub fn take_pending_prompt(&mut self) -> Option<String> {
        if let Ok(val) = self.client.run("harness-pending") {
            let s = val.to_string();
            if !s.is_empty() && s != "nil" {
                let _ = self.client.run("(set harness-pending nil)");
                return Some(s);
            }
        }
        None
    }

    #[cfg(not(feature = "plugin"))]
    pub fn take_pending_prompt(&mut self) -> Option<String> {
        None
    }

    #[cfg(feature = "plugin")]
    pub fn store_response(&mut self, response: &str) {
        let escaped = response.replace('\\', "\\\\").replace('"', "\\\"");
        let _ = self
            .client
            .run(&format!(r#"(set harness-response "{}")"#, escaped));
    }

    #[cfg(not(feature = "plugin"))]
    pub fn store_response(&mut self, _response: &str) {}

    #[cfg(feature = "plugin")]
    pub fn eval(&mut self, code: &str) -> Result<String, String> {
        self.client
            .run(code)
            .map(|val| val.to_string())
            .map_err(|e: JanetError| format!("Janet error: {e}"))
    }

    #[cfg(not(feature = "plugin"))]
    pub fn eval(&mut self, _code: &str) -> Result<String, String> {
        Err("plugin feature not enabled".to_string())
    }

    #[cfg(feature = "plugin")]
    pub fn dispatch(&mut self, hook: &str, context_janet: &str) -> Result<String, String> {
        let names = match self.hooks.get(hook) {
            Some(names) => names.clone(),
            None => return Ok(String::new()),
        };

        let mut results = Vec::new();
        for name in &names {
            let code = format!(
                r#"(do (def ctx {}) ({} ctx))"#,
                context_janet, name
            );
            if let Ok(result) = self.eval(&code) {
                let s = result.to_string();
                // Janet nil -> skip
                if s != "nil" && !s.is_empty() {
                    results.push(s);
                }
            }
        }

        Ok(results.join("\n"))
    }

    #[cfg(not(feature = "plugin"))]
    pub fn dispatch(&mut self, _hook: &str, _context_janet: &str) -> Result<String, String> {
        Ok(String::new())
    }
}
