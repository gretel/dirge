---
name: writing-skills
description: Use when creating, editing, or reviewing a dirge skill — how to structure SKILL.md, write a description that gets the skill loaded at the right time, keep it token-efficient, and verify it actually changes behavior.
---

# Writing Skills

## What a skill is

A skill is an on-demand instruction bundle — a directory with a `SKILL.md` that
the agent loads mid-session via the `skill` tool. dirge lists every skill's
`name` + `description` up front; the agent reads the full body only when a task
matches. Skills live in `.dirge/skills/<name>/` (project or home); project
skills override global ones by name.

**Skills are:** reusable techniques, patterns, and reference guides that apply
across tasks.

**Skills are NOT:** a narrative of how you solved one problem once.

## When to create one (and when not to)

Create a skill when a technique wasn't obvious, you'd reach for it again across
projects, and it involves *judgment* (not a mechanical rule).

Don't create a skill for:

- **Project-specific facts or conventions** → use the `memory` tool, or
  `AGENTS.md`/`CLAUDE.md`. Skills are for broadly-applicable know-how.
- **One-off solutions** → not reusable.
- **Mechanically-enforceable rules** → if a regex/validator/gate can enforce it,
  automate it; save skills for judgment calls.

## The description field is the trigger

The `description` is the only thing the agent sees before deciding to load the
skill, so it's the most important line. Write it as **triggering conditions**,
not a workflow summary (a summary tempts the agent to act on the description
*instead of* reading the skill).

```yaml
# weak — vague, no trigger, summarizes the process
description: Helps with debugging by finding root causes through investigation.

# strong — starts with "Use when", names the situation, no workflow
description: Use when encountering a bug, test failure, or unexpected behavior, before proposing fixes.
```

Cover the words a future agent would actually be thinking ("test failure",
"flaky", "race condition"), and start with **"Use when …"**. Third person, no
first person.

## SKILL.md structure

```markdown
---
name: kebab-case-name
description: Use when <trigger> — <one line on what it gives you>.
---

# Skill Name

## Overview        — the core principle in 1–2 sentences
## When to Use     — concrete triggering situations
## <The content>   — the technique/pattern/checklist itself
## Common Mistakes — the failure modes this prevents (a table works well)
```

Keep it focused on one technique. A table of "excuse → reality" or
"mistake → fix" is a high-density way to close rationalizations.

## Keep it token-efficient

The body loads into context, so every line costs. Be ruthless:

- **Reference, don't reproduce.** Point at `--help`, a man page, or another
  skill instead of pasting their contents.
- **Trim examples** to the shortest form that still teaches.
- **Push heavy detail into supporting files** in the skill directory (e.g.
  `reference.md`) and link them; the agent reads them only if needed.

## Verify the skill changes behavior

A skill that doesn't change what the agent does is dead weight. Check it the way
you'd check a test:

1. **Baseline** — give the task to a subagent (`task` tool) *without* the skill
   and note how it goes wrong (the exact rationalizations it reaches for).
2. **Write the skill** to address those specific failures.
3. **Re-run** the same task with the skill loaded and confirm the behavior
   changed.
4. **Close loopholes** — when the agent finds a new way to rationalize around
   the rule, name that excuse explicitly and re-check.

If you skipped the baseline, you don't know whether the skill teaches the right
thing — only that it sounds reasonable.

## Lifecycle

dirge's post-session skills curator manages skills over time (stale detection,
consolidation, archiving) — so keep each skill single-purpose and well-described
rather than bundling several concerns into one.

---

*Distilled from [superpowers](https://github.com/obra/superpowers) by Jesse
Vincent (MIT License), adapted to dirge's skill system.*
