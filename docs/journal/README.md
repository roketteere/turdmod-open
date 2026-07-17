# Journal — survives session crashes

A three-file operational journal Claude keeps in turdmod, so a session
that crashes mid-task can be resumed cleanly by the next session.

| File | Purpose | Read at session start? |
|---|---|---|
| [WORKSPACE.md](./WORKSPACE.md) | Current task in flight + recently completed. Checklist-style. Append-only history. | **Yes — first thing.** |
| [TOOLS.md](./TOOLS.md) | Catalog of every relevant tool, skill, RE binary, and in-repo script. Reach here before reinventing. | Skim on demand. |
| [BUGS.md](./BUGS.md) | Bug history (open + closed). Symptom, root cause, fix, related memory link. | Skim on demand or when a bug looks familiar. |

## Discipline

- **Update WORKSPACE.md _before_ starting work** — capture intent so a
  crash doesn't lose the plan. Then check off steps as they ship.
- **Never delete history.** Completed tasks move from
  `RIGHT NOW` to `RECENTLY COMPLETED` (newest at top). Closed bugs
  move from `OPEN` to `CLOSED`.
- **Cross-link memory.** When a journal entry resolves into a stable
  fact, write/update the memory at
  `~/.claude/projects/C--Development-claude-turdmod/memory/` and link
  it from the journal with `[[memory-slug]]`.
- **Commit journal updates with the work they describe.** WORKSPACE
  changes go in the same commit (or an adjacent one) as the code that
  flipped a step from `[ ]` to `[x]`.

## What goes where

- Code-change intent / step-by-step → **WORKSPACE.md**
- "I just learned tool X exists at path Y" → **TOOLS.md**
- "I just hit error Z and here's why" → **BUGS.md**
- Stable, reusable facts → memory file (and link from here)
- Forward-looking product ideas → `IDEAS.md` at repo root (not here)
- Approved implementation plan for a multi-step task → `~/.claude/plans/<slug>.md` (link from WORKSPACE.md)
