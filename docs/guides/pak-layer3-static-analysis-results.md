# Pak Layer 3 — Static Analysis Results (2026-05-23)

## Status after this session's sigscan

**L3 architecture is now fully mapped statically.** What's left is identifying
r15's class identity at runtime, which requires either a PolyHook2 probe on
L3 entry or RTTI (stripped in the Shipping build).

## Confirmed via patternsleuth + capstone

### L3 function structure (VA `0x143b61be0` – `0x143b61dae`)

- ~370-byte function. Hash-table lookup + fallback create+register.
- Member access pattern: rdi (this) + 0x70/0x80/0x88 — TMap-shape internal cache (bucket array, capacity, critical section).
- First `xor eax, eax` at `0x143b61c6a` is BENIGN — it's the cache-miss path that falls through to the create-attempt branch.
- The REAL failure: `call qword ptr [rax + 0xc0]` at `0x143b61cac` where `rax = [r15]` (r15's vtable). If null, fall to:
  - `0x143b61d24` — log-verbosity gate
  - `0x143b61d49` — log call with format string `"Unable to create pak \"%s\" handle"` at `.rdata` VA `0x14620cdb0`
  - `0x143b61d6c` — `xor eax, eax; ret`
- The success path at `0x143b61d7a` calls `0x143b6fbf0` (non-virtual) which produces the final pak handle.

### L3 caller (one xref into L3, found via patternsleuth)

Caller function: VA `0x143b531a0` – `0x143b53589` (~905 bytes).
Call to L3 at VA `0x143b5352f`. Call site preamble:

```asm
0x143b531dd:  mov  r15, rdx        ; caller's arg2 stored in r15
...
0x143b531b9:  lea  rax, [14620A2B8h]  ; load caller's vtable address
0x143b531c0:  mov  [rcx], rax         ; write to this->vtable (object construction!)
...
0x143b53510:  mov  rdx, r15        ; pass r15 unchanged as L3 arg2
0x143b5351a:  mov  rcx, rbx        ; this (caller's) as L3 arg1
0x143b5352f:  call 0x143b61be0     ; L3
0x143b53534:  mov  rdi, rax        ; save handle
0x143b53537:  test rax, rax
0x143b5353a:  je   0x143b5356f     ; fail path
0x143b5353c:  mov  r9, qword ptr [r15]
0x143b5353f:  lea  rdx, [rsp+0x60]
0x143b53547:  mov  rcx, r15
0x143b5354a:  call qword ptr [r9 + 0xa0]   ; ANOTHER vcall on r15, slot +0xa0
```

**Crucial finding**: the caller is CONSTRUCTING an object (writes vtable to
`[rcx]`). It then calls L3 with that object as arg1 and `r15` (the caller's
own arg2) as L3's arg2. After L3 returns, it makes a second virtual call on
r15 at slot `+0xa0` — meaning r15 ALSO has a vtable. So r15 is itself a
polymorphic class instance.

### Caller's class vtable (VA `0x14620A2B8`)

8 vtable slots inspected (`patternsleuth`-extracted addresses):

| Slot | Offset | Function VA | Region |
|---|---|---|---|
| 0 | 0x00 | 0x143b56c40 | same-module (pak family) |
| 1 | 0x08 | 0x14090d410 | UE engine code |
| 2 | 0x10 | 0x143b69960 | same-module |
| 3 | 0x18 | 0x141351c40 | UE engine code |
| 4 | 0x20 | 0x143b69980 | same-module |
| 5 | 0x28 | 0x141177a50 | UE engine code |
| 6 | 0x30 | 0x143b605f0 | same-module |
| 7 | 0x38 | 0x143b56eb0 | same-module |

The same-module function addresses (0x143b...) place this in the FPakFile /
FPakPlatformFile family. The interleaved UE-engine slots (0x140917..0x1413..)
are inherited base-class methods — consistent with FPakFile inheriting from
FArchive.

**Best inference**: the caller is `FPakFile` (or a SCUM-derived subclass).
The function at `0x143b531a0` is likely `FPakFile::Initialize` or a near
relative — it constructs the object (sets vtable) then calls L3 to register
the pak handle in the platform-file's cache.

### RTTI is stripped

`vtable[-1]` (the slot before slot 0) at VA `0x14620A2B0` reads as `0x73` —
a small integer, not a valid COL pointer. UE 4.27 Shipping builds strip
`/GR-` RTTI. Standard MSVC `_RTTICompleteObjectLocator` path is closed.

### r15's identity — still unknown statically, knowable at runtime

r15 came from the caller's arg2 (rdx). Static analysis can't trace this
back without finding callers of `0x143b531a0` recursively — multi-hour work.

**Best move**: install a PolyHook2 detour at L3 (`0x143b61be0`) that
captures r15 + reads `[r15]` (its vtable) at entry. Then map the vtable
address to a known class either via:
- `[r15+0x18]` if r15 IS a UObject → class FName via UE reflection
- The vtable address falls in `.rdata` and we can search for adjacent
  symbol strings (UE often keeps class names near vtables for
  `FName::FName(<class_name>)` initializer constants)
- Cross-reference vtable bytes against scumdump's class population by
  scanning GUObjectArray for instances whose vtable matches

### SCUM does NOT use standard PKCS#1 RSA pak signing

Verified prior session. Byte-pattern scans for SPKI / RSAPublicKey /
RSA-2048 header all returned 0 hits. No embedded RSA key. Either:
- Custom signature scheme (HMAC-SHA256 with symmetric key, custom hash
  allowlist, or no signature beyond structural checks)
- Encrypted/obfuscated key blob (e.g., XOR'd, must locate via deeper
  scan)

## Recommended next-session path

1. **Ship `probeL3CallerType` bridge handler** (env-gated TURDMOD_PROBE_L3=1):
   - Installs PolyHook2 detour at `0x143b61be0`
   - On entry: capture `rdx` (r15), read `[r15]` (vtable), read `[r15+0x18]`
     (FName if UObject), record to a thread-local buffer
   - Call original L3
   - Emit `l3.probe` event with captured data

2. **Trigger a pak load**: drop the HelloWorld probe-pak in
   `<SCUM Server>/Content/Paks/` or invoke `runHelloWorld` to force a
   pak-cache lookup.

3. **Read captured r15 type**. With class identity known, look up
   `vtable[0xc0]`'s target in the binary, disassemble, identify what
   condition leads to null return on production paks.

4. **Compare probe-pak vs production-pak code paths** in `vtable[0xc0]`
   — what byte/header check passes for one and fails for the other?

Estimated next-session effort: 1-2 hours for the probe shipment +
hopefully 1-2 hours for the production-pak comparison.

## Key VAs for the next session

| Symbol | VA | Role |
|---|---|---|
| L3 entry | `0x143b61be0` | pak-handle creation function |
| L3 fail epilogue (log) | `0x143b61d49` | "Unable to create pak \"%s\" handle" call |
| L3 success path enter | `0x143b61d7a` | post-vcall, calls handle finalizer at 0x143b6fbf0 |
| The vtable[0xc0] call | `0x143b61cac` | the real gate |
| L3 caller | `0x143b531a0` | likely FPakFile::Initialize |
| L3 caller's vtable | `0x14620A2B8` | identifies the caller's class |
| Log string "Unable to create pak ..." | `0x14620cdb0` | .rdata wide string |

## Summary

L3's mechanics are fully understood. The remaining unknown is r15's class —
solvable with one runtime probe + one pak-load test. Track C is now
**research complete, tooling-shipment one step away**.
