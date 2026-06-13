---
name: systematic-debugging
description: Use when encountering any bug, test failure, or unexpected behavior, before proposing fixes — find the root cause first instead of guessing at symptom patches.
---

# Systematic Debugging

## Overview

Random fixes waste time and create new bugs. Quick patches mask underlying
issues.

**Core principle:** find the root cause before attempting a fix. Symptom fixes
are failure.

## The Iron Law

```
NO FIXES WITHOUT ROOT-CAUSE INVESTIGATION FIRST
```

If you haven't completed Phase 1, you cannot propose a fix.

## When to Use

Any technical issue — test failures, bugs, unexpected behavior, performance
problems, build failures, integration issues. **Especially** when you're under
time pressure, a "quick fix" seems obvious, you've already tried a fix that
didn't work, or you don't fully understand the issue. Don't skip it because a
bug "seems simple" — simple bugs have root causes too, and systematic is faster
than thrashing.

## The Four Phases

Complete each phase before moving to the next.

### Phase 1 — Root-cause investigation

Before attempting any fix:

1. **Read the error carefully.** Don't skip past it. Read the full stack trace;
   note line numbers, paths, error codes. The exact answer is often in there.
2. **Reproduce consistently.** Can you trigger it reliably? What are the exact
   steps? If it's not reproducible, gather more data — don't guess.
3. **Check recent changes.** `git diff` / recent commits, new dependencies,
   config or environment differences. What changed that could cause this?
4. **Instrument component boundaries** (multi-layer systems). Before proposing a
   fix, add logging at each boundary — what data enters, what exits, whether
   config/env propagates. Run once to see *where* it breaks, then investigate
   that component specifically.
5. **Trace data flow backward.** When the error is deep in the call stack, find
   where the bad value *originates*: what passed it in, and what passed it to
   that? Keep tracing up to the source and fix there, not at the symptom.

### Phase 2 — Pattern analysis

1. **Find working examples.** Locate similar code in the same codebase that
   works.
2. **Read references completely.** If you're following a pattern or reference
   implementation, read every line — don't skim and adapt.
3. **List every difference** between the working and broken cases, however
   small. Don't assume "that can't matter."
4. **Understand dependencies** — what config, environment, and assumptions the
   code relies on.

### Phase 3 — Hypothesis and testing

1. **Form one hypothesis.** State it precisely: "I think X is the root cause
   because Y."
2. **Test minimally.** Make the smallest possible change to test it — one
   variable at a time. Don't fix several things at once.
3. **Verify before continuing.** Worked → Phase 4. Didn't → form a *new*
   hypothesis; don't pile fixes on top.
4. **When you don't know, say so.** Don't pretend. State what you've tried and
   ask the user (the `question` tool) with concrete next-step options, or
   research further.

### Phase 4 — Implementation

1. **Write a failing test first.** Simplest reproduction; automated if possible.
   Have it before fixing.
2. **Make one fix** — address the root cause, one change, no "while I'm here"
   refactors.
3. **Verify with fresh output.** Run the test and the surrounding suite; confirm
   the issue is gone and nothing else broke. (dirge's verifier gate will hold
   you to this at finalization — meet it honestly.)
4. **If the fix doesn't work, stop and count.** Fewer than 3 attempts → return
   to Phase 1 with the new information. **3+ failed fixes → question the
   architecture**, don't attempt fix #4.

When 3+ fixes fail and each reveals a new problem elsewhere or demands "massive
refactoring," that's an architectural problem, not a failed hypothesis. Stop and
raise it with the user (the `question` tool) — continue fixing vs. propose a
refactor — rather than grinding on symptoms.

## Red flags — stop and return to Phase 1

If you catch yourself thinking any of these, stop:

- "Quick fix for now, investigate later."
- "Just try changing X and see if it works."
- "Add several changes, then run the tests."
- "It's probably X, let me fix that" (before tracing data flow).
- "I don't fully understand this, but it might work."
- "One more fix attempt" (after 2+ failures).

## Common rationalizations

| Excuse | Reality |
|--------|---------|
| "Too simple to need the process" | Simple bugs have root causes too; the process is fast for them. |
| "Emergency, no time" | Systematic is *faster* than guess-and-check thrashing. |
| "Try this first, investigate later" | The first fix sets the pattern. Do it right from the start. |
| "I'll test after confirming the fix" | Untested fixes don't stick. Test first proves it. |
| "Reference is long, I'll adapt the pattern" | Partial understanding guarantees bugs. Read it fully. |
| "One more attempt" (after 2+ fails) | 3+ failures = architectural problem. Question the design. |

## When investigation finds no root cause

If the issue really is environmental, timing-dependent, or external: you've
completed the process — document what you investigated, add appropriate handling
(retry, timeout, clear error), and add logging for next time. But most
"no root cause" cases are incomplete investigation.

---

*Adapted from [superpowers](https://github.com/obra/superpowers) by Jesse
Vincent (MIT License).*
