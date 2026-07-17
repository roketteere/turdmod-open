# Pak Layer 3 — Round 3 runtime + structural wall (2026-05-23)

Continued from Round 2 (`pak-layer3-round2-class-id.md`). Round 3 ran the
new read-only bridge introspection tools against a live SCUMServer, plus
deeper static analysis. Significant new findings + an honest structural
wall.

## TL;DR

- Bridge shipped 4 new read-only handlers: `readMemory`, `dumpVTable`,
  `imageBase`, `findUInt64`. All page-protection-checked. Deployed via
  autonomous `shutdownServer` + `schtasks` cycle.
- ASLR delta computed: GameServer.exe loads at `0x7ff792360000`
  (preferred base `0x140000000`); delta `0x7ff652360000`.
- L3's actual call signature confirmed:
  ```
  vtable[24]( rcx=r15, rdx=filename, r8=0 )
  ```
  i.e. `r15->vtable[0xc0]( filename, bAllowWrite=false )`.
- **`0x143b531a0` ("inner") is NOT some helper — it's a constructor.**
  It writes `vtable=0x14620a2b8` at `[rcx]` and `vtable+0x08` at
  `[rcx+0x10]` (multi-inheritance secondary vtable). The constructed
  class allocates 0x298 (664) bytes via `FMemory::Malloc`, stores the
  pak path at `[+0x18]`, and many other metadata fields.
- 64 live heap instances of this class were found in process memory
  (search via new `findUInt64` handler). Spacing 0x2c0 (704 bytes).
  The class is the **per-pak metadata class** — most plausibly
  `FPakFile` or a close cousin.
- **The call chain is**:
  ```
  outer fn (0x143b67345, 3116 bytes)            ; FPakPlatformFile::Mount-ish
    → FMemory::Malloc(0x298)
    → inner (constructor of per-pak class)
      → L3 (open-handle gate)
        → r15->vtable[0xc0](filename, false)    ; the actual gate call
  ```
- The outer fn passes `[r13+0x08]` as rdx to inner. r13 is the outer
  function's `this`. So r15 inside L3 = `[outer_this+0x08]`. That
  field is the **`IPlatformFile*` LowerLevel** held by outer_this.
- **Structural wall:** identifying r15's concrete class purely
  statically requires either (a) finding outer_this's vtable so we
  can name the class and look at how it sets LowerLevel, or (b)
  observing rcx at runtime via a hook. Path (a) is many xref hops
  deep; path (b) requires PolyHook2 which is environmentally broken
  here, or a manual JMP patch (proven to risk bridge hang).

## What ships from this round

### Bridge handlers (commit `9a0e389` + this round's incremental updates)

| Handler | Effect | Use |
|---|---|---|
| `readMemory(addr, size)` | page-checked raw read, ≤4 KiB | inspect any memory at any address; refuses unmapped/guard pages |
| `dumpVTable(addr, slots=32)` | reads N × 8-byte function pointers | enumerate a vtable in one call |
| `imageBase()` | returns `GetModuleHandleW(nullptr)` | compute the ASLR delta for static-to-runtime VA translation |
| `findUInt64(value, start, end, maxHits)` | byte-aligned scan over committed regions | locate every reference to a known pointer (e.g. vtable VA) in the live process |

All four read-only. No detours, no patches, no execution diversion.

### Static-analysis scripts (`tmp/` — reusable templates)

- `tmp/disasm-callers.py` — backward classification of rcx-setters at a call site
- `tmp/string-xrefs-callers.py` — .pdata-bounded body scan for `lea` into `.rdata` strings
- `tmp/dump-vtable-slot.py` — vtable enumeration + slot disassembly
- `tmp/disasm-l3.py` — L3 body + FPakPlatformFile vtable slot classification
- `tmp/trace-l3-args.py` — trace arg setup chain from outer → inner → L3
- `tmp/full-inner-l3.py` — focused dump of L3 prologue + inner caller body
- `tmp/inner-writes.py` — every write to `[rbx+N]` (this) in the inner constructor
- `tmp/preceding-rdx.py` — 30 insns leading into any call site to find rdx-setter
- `tmp/xref-to.py` + `tmp/xref-to-pdata.py` — find every caller of any function
- `tmp/find-vtable-refs.py` — search `.rdata`/`.data` for vtable VA references
- `tmp/find-pakfile-refs.py` + `tmp/find-pakfile-leaves.py` — locate functions
  returning the `"PakFile"` literal

These are the canonical templates for assembly-level investigation in
this repo. Memory rule: `[[assembly-when-stuck]]`.

## Findings in detail

### 1. The 5 "callers of 0x143b67320" were the WRONG xref

Round 2 patternsleuth xref pointed at 5 callers of the small (37-byte)
function at `0x143b67320`. They all carried `"Mounting pak file"`-style
log strings, so I read them as FPakPlatformFile methods. But the LARGE
function immediately adjacent — `0x143b67345` (3116 bytes) — is what
actually calls the FPakFile constructor. Outer 0x143b67320 is some
unrelated small thunk; it doesn't lead into L3.

Lesson: when patternsleuth gives multi-hit xrefs, also check the
**enclosing function** size + behavior, not just the call site VA.
Tiny enclosing functions (≤50 bytes) are often thunks or unrelated.
Validate via `.pdata`'s `RUNTIME_FUNCTION` bounds — that's authoritative.

### 2. inner (0x143b531a0) is a constructor

Decisive insns at the prologue:

```
0x143b531b9: lea rax, [rip + 0x26b70f8]  ; rax = vtable_va (0x14620a2b8)
0x143b531c0: mov [rcx], rax                ; this[0x00] = vtable
0x143b531c3: lea rax, [rip + 0x26b70f6]  ; rax = vtable + 0x08
0x143b531ca: mov [rcx + 0x10], rax        ; this[0x10] = sub-vtable
```

Then it allocates zero into many fields (`[rcx+0x08]`, `[rcx+0x18]`,
`[rcx+0x20]`, …) plus engine-version-like constants
(`[rcx+0xb0] = 0x5a6f12e1`, `[rcx+0xb4] = 0xb`) and crypto-hash bytes
near `[rcx+0xd0]`–`[rcx+0xe0]`.

That construction shape — vtable + sub-vtable + filename + crypto
hash fields — matches `FPakFile` in UE 4.27.

### 3. inner is called from exactly ONE site

`tmp/xref-to-pdata.py` (function-aware xref over all 366 208 `.pdata`
entries) found a SINGLE caller of `0x143b531a0`: function
`0x143b67345`, at call site `0x143b6740f`. Setup before that call:

```
0x143b673e7: mov ecx, 0x298           ; FMemory::Malloc size = 664 bytes
0x143b673ec: call 0x1417d1230         ; FMemory::Malloc
…
0x143b67401: mov rdx, [r13 + 8]       ; rdx = outer_this->field+0x08
0x143b67405: mov r8, r14              ; r8 = saved arg2
0x143b6740c: mov rcx, rax             ; rcx = newly malloc'd 664-byte block
0x143b6740f: call 0x143b531a0         ; inner ctor
```

r13 is set in the outer prologue:

```
0x143b67354: mov r13, rcx             ; r13 = outer's this
```

So **r15 inside L3 = [outer_this + 0x08]** — the IPlatformFile field
of whatever class owns `0x143b67345`.

### 4. Runtime heap revealed 64 live FPakFile-ish instances

Used new `findUInt64` to scan the whole process for the runtime VA of
vtable `0x14620a2b8`. Found 64 hits, clustered at 0x2c0 spacing.
Reading one returned the expected layout: vtable at +0x00, flag 0x1
at +0x08, sub-vtable at +0x10, wide-string filename at +0x18 (e.g.
`"../../../SCUM/Co…"`), various ints, then a pointer at +0x30 to
another object whose vtable is at `.rdata 0x14620A928`.

### 5. The "PakFile"-returning leaf is unique

Scanned all of `.text` byte-by-byte for the pattern
`48 8d 05 <imm32> c3` (= `lea rax, [rip+imm32]; ret`) where imm32
resolves to wide string `"PakFile"` at `0x144e8aed0`. **Exactly one
match: `0x143b605e0`** — slot 24 of vtable `0x14620a2b8`. So no other
class in the binary has a leaf `GetName`/`GetTypeName` returning
`"PakFile"`. FPakPlatformFile (if it has GetName) must use a longer
non-leaf body, or share this same function via vtable aliasing (but
`.rdata` has only ONE 8-byte-aligned reference to `0x143b605e0`).

### 6. None of the IPlatformFile-derived class names are in the binary

Checked for `"WindowsPlatformFile"`, `"CachedReadPlatformFile"`,
`"LoggedPlatformFile"`, `"NetworkPlatformFile"`, `"SignedPlatformFile"`
etc. — zero hits in both ASCII and UTF-16. Either SCUM doesn't use
the standard UE IPlatformFile derivatives (custom replacement), or
the class identifier strings are stripped from the shipping build,
or my slot-numbering for `GetName` is off (slot 24 might not be
GetName despite what the literal suggests).

## The structural wall

To finish the L3 crack, we need to identify r15's concrete class so
we can:
1. Locate its vtable (different from 0x14620a2b8)
2. Read what its slot 24 (=vtable[0xc0]) actually does
3. Determine whether it's `OpenRead`, a SCUM-custom signature check,
   or something else
4. Design a bypass appropriate to that function's role

Three paths to identify r15's class. Each has a cost:

| Path | Method | Cost | Risk |
|---|---|---|---|
| **A — recursive static xref** | Identify `0x143b67345`'s class via its callers' string xrefs; then locate its vtable in .rdata; then trace how it sets `[this+0x08]`. | many hops, several hours per hop, dead-ends likely if SCUM strips strings | zero runtime |
| **B — PolyHook2 hook on inner** | Detour inner's prologue, capture rdx (= r15) into a slot the bridge can read. | proven environmentally broken here — PolyHook2 returned false on previous attempts at L3 and the inner caller too. | session-killer if hook installs but trampoline is malformed |
| **C — manual JMP patch + log** | Write a 14-byte JMP at inner's entry to a Dll-allocated logger trampoline. | the only no-PolyHook2 dynamic path. proven once before to hang the bridge thread; we have recovery via elevated installer. | bridge thread hang → needs full restart |
| **D — runtime trigger + observe** | Mount a NEW pak via Notifications.json reload or RegisterEncryptionKey — observe whether vanilla L3 succeeds with the path we provide. | requires SCUM to actually re-run mount logic; vanilla SCUM doesn't expose "remount" RPC | low — read-only observation |

The cheapest unblock is **Path D** combined with a **probe pak**:
authore a 1-class UE 4.27 BP pak, drop it in `Content/Paks/`, restart,
observe whether L3 paths succeed or fail. The runtime error log
fingerprints the failure precisely.

## Next round shape

**Recommended:** unblock by **switching to the cooking side**.

1. Use UE 4.27.2 Editor (already installed on Joel's machine) to
   create a minimal blueprint pak: a single UBlueprintFunctionLibrary
   subclass with one BlueprintCallable static method.
2. Cook for `WindowsServer` target. Output `.pak` lands in
   `<Project>/Saved/StagedBuilds/WindowsServer/<Project>/Content/Paks/`.
3. Drop into SCUM's `Content/Paks/` (alongside an `.sig` companion —
   may need to forge or copy from a vanilla pak).
4. Restart SCUM with new pak.
5. Observe: does L3 print "Unable to create pak handle"? Does
   the v3.1 file-flag bypass fire? What's the EXACT failure mode?
6. The failure mode tells us which validator to crack first —
   which can then guide hooking (or makes the crack unnecessary if
   the pak happens to load through Joel's existing v3.1 / v4 / v5
   suppressors).

## Late-round finding: `FWindowsPlatformFile` candidate vtable located

After the structural-wall section was written, one more dig produced a
concrete result that may reframe the whole crack:

**Candidate `FWindowsPlatformFile` vtable @ `.rdata 0x145de4070`** (32+
slots, runtime VA `0x7ff798144070`).

Located by scanning all `.text` functions for `call qword ptr
[rip+N]` where N resolves to the import-address-table slot for
`CreateFileW`. 17 candidate functions emerged; 6 of them are
referenced from `.rdata` at 8-byte-aligned positions (i.e. they're
vtable slots). 5 of the 6 cluster within a 128-byte window —
characteristic of consecutive IPlatformFile virtuals (OpenRead,
OpenReadNoBuffering, OpenWrite, DirectoryExists, CreateDirectory,
DeleteDirectory, etc.) all within one class's vtable.

Reading 32 slots starting at the inferred vtable base
(`0x145de4130 - 0xc0 = 0x145de4070`) yielded a clean IPlatformFile
shape:

| Slot | Static VA | Bytes | Role |
|---|---|---|---|
| 0 | `0x142837c50` | (function) | vector deleting dtor |
| 24 (0xc0) | `0x14283a3d0` | 728 bytes, calls CreateFileW at `0x14283a447` | **`OpenRead`** ✓ |
| 27 (0xd8) | `0x142837840` | 124 bytes, CreateFileW | adjacent file-op virtual |
| 28 (0xe0) | `0x142837a20` | 128 bytes, CreateFileW | adjacent file-op virtual |
| 29 (0xe8) | `0x142837b40` | 231 bytes, CreateFileW | adjacent file-op virtual |
| 40 (0x140) | `0x142837680` | 156 bytes, CreateFileW | another file-op virtual |

Disassembly of slot 24 (function `0x14283a3d0`):

```
0x14283a3d0: mov [rsp+8], rbx
0x14283a3d5: mov [rsp+0x20], rbp
…
0x14283a407: call 0x14283b0b0       ; path normalization helper
0x14283a447: call qword ptr [rip + 0x25fc413]   ; CreateFileW from IAT
0x14283a455: test rcx, rcx          ; check for null prior allocation
…
0x14283a45f: cmp rsi, -1            ; INVALID_HANDLE_VALUE check
0x14283a463: je 0x14283a637         ; failure path
```

That's textbook `FWindowsPlatformFile::OpenRead`. Confidence: high.

**Implication.** IF `FPakPlatformFile::LowerLevel` in vanilla SCUM
is `FWindowsPlatformFile` directly (no signed/wrapper class between
them), then L3 is **NOT a validator — it's just a file-existence
check**. Any pak file at a valid filesystem path would pass through
L3 unchanged. The pak-loading failures we've worked around (v1
file-existence, v2/v4 signature integrity, v5 extra-pak modal) are
the REAL gates; L3 happens AFTER them and just opens the file.

This hypothesis is testable: cook a probe pak, drop it in
`Content/Paks/`, restart SCUM with v3.1/v4/v5 active. If the pak
loads (or fails on a DIFFERENT layer), L3 was never really blocking
us.

**Open uncertainty:** SCUM might use a signed-platform-file wrapper
between FPakPlatformFile and FWindowsPlatformFile. We found only
ONE heap address (in a volatile stack region) referencing the
`FWindowsPlatformFile` vtable — no clear singleton instance pinned
in heap. That could mean (a) FWindowsPlatformFile IS the LowerLevel
but it's stack-resident or referenced only in callgraph paths, or
(b) SCUM doesn't use FWindowsPlatformFile as LowerLevel at all —
some other class with a CreateFileW-based OpenRead is the actual
LowerLevel.

Verifying which requires either:
- Reading the runtime FPakPlatformFile instance's `[+0x08]` field
  directly (need to find the FPakPlatformFile singleton — not yet
  located in heap)
- OR cooking a probe pak and observing the failure mode

## Risk posture

This round: zero runtime crashes, two clean autonomous deploy
cycles (`shutdownServer` → copy DLL → `schtasks /Run` → bridge
reconnect). All scans were read-only. Bridge is stable on the
new build.

The recommended next-round move (cook a probe pak + observe)
keeps the same zero-deploy-risk posture.
