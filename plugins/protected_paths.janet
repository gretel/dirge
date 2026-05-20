# Protected paths plugin
#
# Demonstrates the Phase 1 plugin tool-hook APIs:
#   (harness/block "reason")          - abort the tool call before it runs
#   (harness/mutate-input json-string) - rewrite the tool args
#   (harness/replace-result string)   - swap the tool output the LLM sees
#
# This plugin blocks writes/edits to a small set of sensitive paths so
# the agent can't accidentally clobber secrets or version-control state.
# Ported from the pi `protected-paths.ts` example.

(def hooks ["on-tool-start" "on-tool-end"])

(def protected-substrings [".env" ".git/" "node_modules/" "/secrets/"])

(defn- args-path [args-json]
  # The args are a JSON-encoded string; we don't run a full JSON parser
  # here, just look for the literal `"path": "<value>"` substring. Good
  # enough for the built-in write/edit/apply_patch tools.
  (when args-json
    (let [marker "\"path\""
          mark-pos (string/find marker args-json)]
      (when mark-pos
        (let [after (string/slice args-json (+ mark-pos (length marker)))
              q1 (string/find "\"" after)]
          (when q1
            (let [rest (string/slice after (+ q1 1))
                  q2 (string/find "\"" rest)]
              (when q2 (string/slice rest 0 q2)))))))))

(defn- path-is-protected? [path]
  (var hit false)
  (loop [p :in protected-substrings]
    (when (string/find p path) (set hit true)))
  hit)

(defn on-tool-start [ctx]
  (let [tool (ctx :tool)
        args (ctx :args)]
    (when (or (= tool "write") (= tool "edit") (= tool "apply_patch"))
      (let [path (args-path args)]
        (when (and path (path-is-protected? path))
          (harness/block (string "write to protected path '" path "' refused"))))))
  nil)

(defn on-tool-end [ctx]
  # Example: truncate very long tool outputs so the LLM sees a compact
  # summary instead of pages of noise. Only triggered for very large
  # results; small results pass through unchanged.
  (let [output (ctx :output)]
    (when (and output (> (length output) 4000))
      (harness/replace-result
        (string (string/slice output 0 4000)
                "\n... [truncated by protected_paths plugin: "
                (- (length output) 4000)
                " more chars]"))))
  nil)
