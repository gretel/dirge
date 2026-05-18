(defn on-init [ctx]
  (string "loaded with " (ctx :model)))

(defn on-prompt [ctx]
  (let [prompt (ctx :prompt)]
    (if (string/find "hello" prompt)
      "greeting detected"
      nil)))

(defn on-response [ctx]
  (let [resp (ctx :response)]
    (if (string/find "error" resp)
      "error in response"
      nil)))
