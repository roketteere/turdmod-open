# Pak Layer 3 — Round 2 class-identification results (2026-05-23)

Static analysis run on `GameServer.exe` (124,348,416 bytes, 8 sections,
`.text` at VA 0x140001000, image base 0x140000000). All findings are
from python+capstone disassembly directly over the PE — no live RPCs,
no PolyHook2, no dynamic risk.

## TL;DR

| Question | Answer | Confidence |
|---|---|---|
| What class owns function `0x143b67320`? | `FPakPlatformFile` | high |
| Where is the FPakPlatformFile vtable? | `.rdata 0x14620A2B8` (32+ slots) | high |
| What is `0x143b605e0`? | `FPakPlatformFile::GetName()` returning `"PakFile"` | high |
| What is `r15` inside L3 (`0x143b61be0`)? | `FPakPlatformFile::LowerLevel` (pointer at `[this+0x08]`) | high |
| What does L3 actually dispatch? | `LowerLevel->vtable[0xc0]` — slot 24 of LowerLevel's vtable (NOT FPakPlatformFile's) | high |
| Is LowerLevel `FWindowsPlatformFile`? | **unknown** — next round identifies | unverified |
| What does that gate function do? | **unknown** — next round disassembles | unverified |

## How we got here

### Step 1 — find callers of 0x143b67320

patternsleuth xref of 0x143b67320 returned 5 call sites:
0x143b62740, 0x143b6707d, 0x143b67308, 0x143b689e8, 0x143b6c450.

### Step 2 — classify what each caller passes as `this` (rcx)

Script: `tmp/disasm-callers.py`. For each call site, disassemble a 96-byte
backward window, locate the `call 0x143b67320` insn, then walk back up to
32 insns looking for the most recent write to rcx/ecx. Result table:

| Call site | rcx setter | r9 setter (always next) |
|---|---|---|
| 0x143b62740 | `mov rcx, rbp` | `movzx r9d, byte ptr [rbp + 0x30]` |
| 0x143b6707d | `mov rcx, r13` | `movzx r9d, byte ptr [r13 + 0x30]` |
| 0x143b67308 | `mov rcx, <passthrough>` | `movzx r9d, byte ptr [rcx + 0x30]` |
| 0x143b689e8 | `mov rcx, rsi` (rsi from `[rbp-0x78]`) | `movzx r9d, byte ptr [rsi + 0x30]` |
| 0x143b6c450 | `mov rcx, r10` | `movzx r9d, byte ptr [r10 + 0x30]` |

Pattern is identical across all five callers: pass `this` as rcx, then
read `[this + 0x30]` as a uint8 and pass as r9 (4th arg). The byte at
+0x30 is a per-pak boolean flag (likely `bSigned` / `bEncrypted`).

### Step 3 — identify the class via string xrefs in caller bodies

Script: `tmp/string-xrefs-callers.py`. Walk `.pdata` to find the enclosing
function bounds for each call site, then scan that function's body for
RIP-relative LEAs into `.rdata` and resolve the ASCII/wide strings.

```
=== call site 0x143b62740 ENCLOSING fn :: 0x143b626d0 .. 0x143b62750 (128 bytes) ===
  0x143b626fa  lea rcx     ->  0x14620d570  [W] 'Mounting pak file: %s \n'

=== call site 0x143b6707d ENCLOSING fn :: 0x143b66e6a .. 0x143b672a7 (1085 bytes) ===
  (no .rdata string references found)

=== call site 0x143b67308 ENCLOSING fn :: 0x143b672f0 .. 0x143b67311 (33 bytes) ===
  (no .rdata string references found — it's a thunk)

=== call site 0x143b689e8 ENCLOSING fn :: 0x143b68490 .. 0x143b68ba2 (1810 bytes) ===
  0x143b68526  lea r8      ->  0x14620d480  [A] '*.pak'
  0x143b68800  lea rdx     ->  0x14620d490  [W] 'Found Pak file %s attempting to mount.'
  0x143b68961  lea rax     ->  0x14620d4e0  [W] 'Pak file %s already exists.'
  0x143b6898a  lea rax     ->  0x14620d518  [W] 'Mounting pak file %s.'
  0x143b689ac  lea rdx     ->  0x14620d548  [A] 'Pak_Mount'
  0x143b68a00  lea rdx     ->  0x14620d490  [W] 'Found Pak file %s attempting to mount.'

=== call site 0x143b6c450 ENCLOSING fn :: 0x143b6c3cb .. 0x143b6c81b (1104 bytes) ===
  0x143b6c3de  lea rbx     ->  0x14620d5e0  [W] "Successfully mounted deferred pak file '%s'"
  0x143b6c5b2  lea rbx     ->  0x14620d5e0  [W] "Successfully mounted deferred pak file '%s'"
  0x143b6c75d  lea rax     ->  0x14620d640  [W] "Failed to mount deferred pak file '%s'"
  0x143b6c7ce  lea rax     ->  0x14620d690  [W] "Registered encryption key '%s': %d pak files mounted, %d remain pending"
```

Every string above is a near-verbatim match for a `UE_LOG(LogPak, ...)`
call in UE 4.27's `Engine/Source/Runtime/PakFile/Private/IPlatformFilePak.cpp`:

- `"Mounting pak file: %s"` — `FPakPlatformFile::Mount`
- `"Found Pak file %s attempting to mount."` — `FPakPlatformFile::MountAllPakFiles`
- `"Pak_Mount"` — LLM (`Low-Level Memory`) stat scope name
- `"Successfully mounted deferred pak file '%s'"` — `FPakPlatformFile::RegisterEncryptionKey` (the deferred-mount path)
- `"Registered encryption key '%s'..."` — same

That **proves** `0x143b67320` is a member of `FPakPlatformFile`.

### Step 4 — locate the FPakPlatformFile vtable

Earlier sessions had a vtable at `.rdata 0x14620A2B8` flagged as
"L3-related". `tmp/dump-vtable-slot.py` dumps the first 32 slots and
disassembles a chosen slot.

Slot 24 (byte offset 0xc0) = `0x143b605e0`. Disasm:

```
0x143b605e0: lea rax, [rip + 0x132a8e9]
0x143b605e7: ret
```

The RIP-relative target resolves to wide-string `"PakFile"` at
`0x144e8aed0`. That's `FPakPlatformFile::GetName()` — the standard
`IPlatformFile::GetName` override every platform-file derivative
implements. So the vtable at `0x14620A2B8` is the `FPakPlatformFile`
vtable, and the prior "8 slots" claim was wrong (the table is 32+ slots
of valid `.text` pointers).

### Step 5 — re-interpret L3's dispatch

R1's earlier analysis stated L3 does:

```
0x143b61cac: call qword ptr [rax + 0xc0]
```

where `rax = [r15]` (load r15's vtable) and r15 came from caller arg2.

If r15 were `FPakPlatformFile`, that call would be `GetName()` — which
returns a pointer to `"PakFile"` and **cannot return null**. The
"vtable[0xc0] returns null → fail" narrative cannot match this slot.

The chain we walked from L3 backward:

```
L3 (0x143b61be0) gets r15 from rdx
  ← inner caller (0x143b531a0) passes its rdx through
    ← outer caller (0x143b67320) passes [r13+8] where r13=this=FPakPlatformFile
      ← all 5 callers pass FPakPlatformFile in rcx
```

Therefore **r15 = `[FPakPlatformFile + 0x08]` = `FPakPlatformFile::LowerLevel`**.

That's the `IPlatformFile*` field every `IPlatformFile` wrapper holds
to chain to its delegate. The vtable L3 dereferences is **LowerLevel's
vtable**, not FPakPlatformFile's. Slot 24 (byte offset 0xc0) of an
`IPlatformFile`-derived vtable is **`OpenReadNoBuffering`** (or
adjacent OpenRead variant — exact slot index depends on whether the
deleting-destructor occupies one or two slots in this build).

That matches the "null return for production paks" pattern: an
`OpenRead`-family virtual returns null for files that don't exist on
disk *or* (if LowerLevel is a SCUM custom wrapper) for files that fail
some integrity check.

## What's next (Round 3)

1. **Identify LowerLevel's class.** Either:
   - Static: disassemble `FPakPlatformFile::Initialize` (vtable slot 1
     or 2) and find what class is `new`'d and stored at `[this+0x08]`.
   - Runtime: once the new bridge DLL (with `readMemory` + `dumpVTable`,
     committed `9a0e389`) is deployed, dereference any FPakPlatformFile
     instance's +0x08 field, then `dumpVTable` LowerLevel's vtable.
2. **Disassemble LowerLevel's slot 24** = the gate function. Its body
   tells us exactly what condition is checked.
3. **Decide bypass shape.** Three candidates ranked by reversibility:
   - **Hook the gate to return non-null** for production pak paths
     (matches v3.1 file-flag gate pattern — env-gated, low risk).
   - **Patch L3's branch** at `0x143b61cb1` (the `test rax, rax; jz`
     that handles the null return) to force the non-fail path.
   - **Forge a valid `.sig` file** alongside the probe pak so the
     gate's underlying check passes naturally (this is the "no
     hooks, no patches" gold-plated route — requires reversing the
     sig format).

Each round is one focused work block. No multi-round speculation in a
single sitting.

## Tooling

All static-analysis scripts live in `tmp/`:

- `tmp/disasm-callers.py` — backward-walk classification of rcx-setters
- `tmp/string-xrefs-callers.py` — .pdata-bounded body scan for string refs
- `tmp/dump-vtable-slot.py` — vtable enumeration + slot disassembly

These are the canonical templates for assembly-level investigation in
this repo. Reuse + tweak rather than re-authoring from scratch.

## Risk posture

This round's work was **zero-risk** — all reads of the on-disk PE, no
process attachment, no SCUM downtime. The new `readMemory` /
`dumpVTable` bridge handlers (commit `9a0e389`) are also read-only and
page-protection-checked. The first dynamic step that touches the
running server (deploying the new DLL) carries normal deploy-cycle
risk only.
