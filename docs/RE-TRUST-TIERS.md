# RE Trust Tiers — the verification convention (read before trusting any RE claim)

**Why this exists:** RE/engine work mixes ground truth with cheap-LLM inference, and the
two look identical on the page. In one session, wrong function names (`ServerChat`), a
stale "chat in-progress" (it was shipped), an "ConZCharacter holds health" inference (the
dump says no), and a 26/73-vs-101 handler count all slipped through because an **inference
was cited as a fact with no verification step.** This convention makes the difference
explicit and checkable.

## The tiers — and the one rule

| Tier | Source | Confirms |
|---|---|---|
| **T1-live** | running engine (named pipe / OVH `/engine/rpc`) | existence, signature, live values, **behavior** (observed) |
| **T1-source** | the actual repo source (bridge `.cpp`: `regs[]`, `kTargets[]`) | **what's implemented** |
| **T2-dump** | `scumdump/data/extracted/<build>/classes.json` | static structure: existence, parent, **offset**, signature |
| **T3-inference** | DeepSeek dossiers, L5/L6 digests, lobe semantic recall | **proposes candidates ONLY — never confirms** |

**THE RULE:** *T3 proposes; T2/T1 dispose.* An inference is a lead, not a fact. It becomes
a fact only when a verifying tier confirms it. **Behavior claims are never "true" from
inference** — they need a live experiment, or they're marked `NEEDS_LIVE`.

## How to apply it
- **Before writing code** off any digest/dossier/inference, run the load-bearing claims
  through `tools/verify/verify-claim.mjs`. **Hard gate:** no implementation unless the
  claim is `VERIFIED_TRUE` (or `NEEDS_LIVE` → then observed on a TEST engine, never prod).
- **In docs**, every "X is implemented / X works" claim cites its source (`file:symbol`).
  Re-verify on touch. (SHIP-STATUS drifted because it was written from memory.)
- **For facts, query the verified lobe project** (`scumdump-gamedata`) before the inferred
  one (`scumdump-l5`); treat `scumdump-l5`/L6 as "where to look," not "what's true."
- **Label provenance** on generated artifacts. Dossiers self-header `HYPOTHESIS`; keep it.
- **Conflicts are loud.** If two tiers disagree, that's `CONFLICT` — surface it, never
  silently pick one.

## What "objective truth every time" actually means
Structural claims (existence / offset / signature / is-implemented) get a definitive
offline verdict. Behavioral claims get a definitive **`NEEDS_LIVE` + the exact probe** —
the system scaffolds the experiment but the truth comes from running it. So the promise is
**a definitive verdict every time, including an honest "not provable without a live test"**
— never a guess wearing a fact's clothes.

Tooling: `tools/verify/` (see its README). The canonical lesson: `--field ConZCharacter.Health`
→ `NOT_FOUND`, so we did **not** author `EV_DEATH` against a field that doesn't exist.
