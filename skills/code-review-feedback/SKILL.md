---
name: code-review-feedback
description: Use when receiving code-review feedback, before implementing suggestions — especially when feedback seems unclear or technically questionable. Verify against the codebase and reason about each item instead of agreeing performatively or implementing blindly.
---

# Receiving Code Review

## Overview

Code review is a technical evaluation, not an emotional performance.

**Core principle:** verify before implementing, ask before assuming, technical
correctness over social comfort.

## The response pattern

```
1. READ      — the whole review, without reacting
2. UNDERSTAND — restate each item in your own words (or ask if unclear)
3. VERIFY    — check it against codebase reality
4. EVALUATE  — is it technically sound for THIS codebase?
5. RESPOND   — technical acknowledgment, or reasoned pushback
6. IMPLEMENT — one item at a time, test each
```

## No performative agreement

Don't open with "You're absolutely right!", "Great point!", "Thanks for
catching that!", or any gratitude/praise filler. State the fix instead, or just
make it — the code shows you heard the feedback.

- Acknowledge a correct item with the change: `Fixed — <what changed> in <where>`.
- If you catch yourself about to type "Thanks" or "Good point," delete it and
  state the fix.

## Handle unclear feedback before implementing anything

If any item is unclear, stop — don't implement the clear ones yet. Items are
often related, and partial understanding produces the wrong implementation. Ask
for clarification (the `question` tool), presenting your interpretations as
options.

> "I understand items 1, 2, 3, and 6. I need clarification on 4 and 5 before
> implementing."

## Evaluate external feedback skeptically

Feedback from a human reviewer or an automated reviewer is a *suggestion to
evaluate*, not an order to follow. Before implementing, check:

1. Is it technically correct for **this** codebase and stack?
2. Does it break existing functionality?
3. Is there a reason the current implementation is the way it is?
4. Does it hold across the platforms/versions you support?
5. Does the reviewer have the full context?

If a suggestion seems wrong, **push back with technical reasoning** — reference
the tests/code that show it. If you can't verify it, say so: "I can't verify
this without X — should I investigate, ask, or proceed?" If it conflicts with a
prior decision the user made, stop and raise that first.

## YAGNI check

If a reviewer asks you to "implement this properly," grep for actual usage
first. If nothing calls it: "This isn't called anywhere — remove it (YAGNI)?"
If it is used, then implement it properly.

## Implementation order

1. Clarify everything unclear first.
2. Then: blocking issues (breakage, security) → simple fixes (typos, imports) →
   complex fixes (refactors, logic).
3. Test each fix individually; verify no regressions.

## If you pushed back and were wrong

State it factually and move on — no long apology, no defending why you pushed
back:

> "You were right — I checked X and it does Y. Implementing now."

## GitHub review threads

When replying to inline review comments, reply *in the comment thread*
(`gh api repos/{owner}/{repo}/pulls/{pr}/comments/{id}/replies`), not as a
top-level PR comment.

## The bottom line

Verify. Question. Then implement. Technical rigor over performative agreement.

---

*Adapted from [superpowers](https://github.com/obra/superpowers) by Jesse
Vincent (MIT License).*
