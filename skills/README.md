# Bundled skills

A small starter pack of general-purpose workflow skills. These are
[Claude-compatible skills](../docs/skills.md): each is a directory with a
`SKILL.md` the agent loads on demand via the `skill` tool, keyed off the
`description` field.

They are **not** installed automatically. To use one, copy its directory into a
skills location dirge discovers — `.dirge/skills/` (or `.claude/skills/` /
`.opencode/skills/`) in your project or home directory:

```sh
# per project
cp -r skills/systematic-debugging .dirge/skills/
# or globally for every project
cp -r skills/code-review-feedback ~/.dirge/skills/
```

dirge picks them up at the next startup and lists them in the `skill` tool.

## What's here

| Skill | Loads when |
|---|---|
| [`systematic-debugging`](systematic-debugging/SKILL.md) | hitting a bug, test failure, or unexpected behavior — find root cause before fixing |
| [`code-review-feedback`](code-review-feedback/SKILL.md) | acting on review feedback — verify and reason before implementing, no performative agreement |
| [`writing-skills`](writing-skills/SKILL.md) | authoring or editing a skill |

## Overlap with dirge's built-ins

dirge already enforces several disciplines at the loop level — the verifier
gate (verify before done), the cross-turn failure-recovery checkpoint, the
`/plan` phased workflow, and the goal gate. These skills complement those with
*proactive* guidance the model reaches for itself; they don't replace the
gates.

## Attribution

`systematic-debugging` and `code-review-feedback` are adapted from the
[superpowers](https://github.com/obra/superpowers) project by Jesse Vincent,
used under the MIT License. `writing-skills` distills superpowers' skill-authoring
guidance for dirge's skill system. The originals were vendored via
[MiMo-Code](https://github.com/XiaomiMiMo/MiMo-Code); adapted here to dirge
conventions (no cross-skill orchestration runtime, dirge-native tooling).
