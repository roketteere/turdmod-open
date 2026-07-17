# Pak Layer 3 Research – Iteration 2
**Next‑Move Synthesis after Sigscan Session (2025-01-14)**

## 1. What We Now KNOW

### 1.1 L3 Confirmed as Pak‑Handle Creation Function
- Identified at RVA `0x03b61be0` (VA `0x143b61be0`).
- The log string `"Unable to create pak \"%s\" handle"` (at `.rdata` VA `0x14620cdb0`) is emitted only on the failure epilogue at `0x143b61d49`. This irrefutably ties L3 to the creation of a `FPakFile` (or equivalent) handle from a path string.

### 1.2 L3 Control Flow Structure
- **Cache hit / try load / fail with log** pattern:
  - `0x143b61bf2‑c17` – Hash lookup of the pak name against an internal `TMap<FName, FPakFile*>`. The map data structures reside at `[rdi+0x70]` (buckets), `[rdi+0x80]` (capacity), `[rdi+0x88]` (critical section).
  - Cache miss falls through to `0x143b61c6a` (first `xor eax, eax` is benign).
  - At `0x143b61c88` the ‘try‑load’ branch begins. Three sub‑paths based on `rdi+0x20` (a flag, likely related to encryption or signing requirements).
  - **The real bottleneck** is at `0x143b61cac`:  
    `call qword ptr [rax + 0xc0]` where `rax = [r15]` – this is a vtable call on the second argument (`r15`). If this call returns `NULL`, we fall to `0x143b61d24` (the failure block). If it returns non‑null, control proceeds to `0x143b61d7a` and then a non‑virtual call at `0x143b6fbf0`. **Our probe pak succeeds through both calls; production paks do not.**

### 1.3 No Standard RSA Public Key
- Byte‑pattern scans for:
  - `SubjectPublicKeyInfo` SEQUENCE header → 0 hits
  - `RSAPublicKey` direct → 0 hits
  - RSA‑2048 exact header → 0 hits
  - `02 03 01 00 01` (exponent 65537) → 2 hits, both in data tables (e.g., game code constants, not actual keys)
- **Conclusion**: SCUM does not use PKCS#1 RSA for pak signing. The signature scheme is likely:
  - SHA‑256 hash allowlist (embedded table of content hashes)
  - HMAC with a symmetric key (possibly derived or obfuscated)
  - Custom obfuscation / no signature (only structural validation)
  - Or a hybrid that uses a different public‑key algorithm (e.g., ECDSA) – but no ECC identifiers were found either.

### 1.4 Three Actionable Attack Surfaces
| Surface | Location | Nature |
|---------|----------|--------|
| **A** | `vtable[0xc0]` of `arg2 (r15)` | Virtual method on what is almost certainly an `IPlatformFile` subclass. This call MUST return a valid handle for the pak to be accepted. |
| **B** | Non‑virtual call at `0x143b6fbf0` | Called after virtual method succeeds. May perform content‑level validation (hash/signature) or simply prepare internal data structures. |
| **C** | Signature scheme itself | Absence of RSA means we need to find what integrity check is applied. Likely in one of the above two surfaces, or in a function called by them. |

All three are still alive, but **surface A is the most immediate barrier** – it is the first check that fails for production paks. Understanding why it fails will unlock the rest.

---

## 2. Which Surface to Investigate NEXT – Decisive Ranking

**Priority: Surface A > Surface B > Surface C**

### Why Surface A?
- It is the **first hard check** after cache miss. The probe pak passes it, production paks fail. Therefore, whatever this virtual method does is the gatekeeper.
- It is a **single identifiable call** with a known offset (vtable slot 24, since `0xc0 / 8 = 24`). We can disassemble its implementation and trace its return logic.
- It likely corresponds to an `IPlatformFile` method such as `OpenRead`, `OpenAsyncRead`, `OpenMapped`, or a **custom method added by SCUM** to a derived class. If it is a custom override, understanding its purpose (e.g., signature check on file open, format validation) gives us the exact bypass.
- **Information gain per hour is highest**: we can immediately decompile the vtable target, read its code, and compare the behaviour between probe vs production pak.

### Surface B (2nd Priority)
- The non‑virtual call at `0x143b6fbf0` runs *after* the virtual call succeeds. It may be where the actual content validation happens (e.g., reading the pak’s signature block, comparing against a hash table). However, if the virtual call already rejects the production pak, surface B is never reached for those. We only need to look at B after we fix A (or if we discover A is trivially bypassed and B is the real check).

### Surface C (Deferred)
- Searching for the signature/hash validation scheme is valuable but more open‑ended. Without knowing exactly where it lives, we risk time‑consuming spelunking. Once we understand the two functions in A and B, they will almost certainly reveal the scheme. Leave C for later, or as a side activity during live tracing.

---

## 3. EXACT Next Sigscan / Disassembly Moves

All addresses are relative to the `.text` base (RVA unless noted otherwise). Use `capstone` or `Ghidra` to disassemble the following functions.

### Move 1: Resolve the vtable call target (Surface A)
- **Address**: `0x143b61cac` – the `call qword ptr [rax + 0xc0]` instruction.
- **What to do**:
  1. At this point, `r15` holds `arg2`. We need to know the type of `arg2`. Open `scumdump/data/extracted/v23128915/classes.json`. Search for classes that inherit from `IPlatformFile` or `FIOSPlatformFile` etc. Look for **SCUM‑specific subclasses** that might override a virtual method. Common candidates: `SCUMPakPlatformFile`, `SCUMIntegrityPlatformFile`, etc.
  2. Cross‑reference the vtable offset 24 (0xc0/8) with the UE 4.27 `IPlatformFile` vtable layout. Standard methods:
     - 0: Destructor (index 0)
     - 1: `GetName`?
     - ... well, UE source shows:
       - vtable[0] = ~IPlatformFile
       - vtable[1] = `ShouldBeUsed`
       - vtable[2] = `SetNextProvider`
       - vtable[3] = `GetNextProvider`
       - vtable[4] = `Initialize`
       - vtable[5] = `Tick`
       - vtable[6] = `UnRegisterRedundantFileSystem`
       - vtable[7] = `GetLowerLevel`
       - ...
       - vtable[24]? Might be `OpenRead` (typically around index 29 in UE 4.27). Let’s confirm: In `GenericPlatformFile.h`, `OpenRead` is virtual function number something like 28? We need the exact order from a UE 4.27 build. **Best bet: disassemble the vtable of the object pointed to by `r15` at runtime** (see live trace below). But for now, we can assume it is either `OpenRead`, `OpenAsyncRead`, or a custom method.
  3. **Disassemble the function at the target address directly**: set a breakpoint, get the actual call target from a live run (see §4). Or, we can static‑analyze the call:
     - The instruction `call qword ptr [rax+0xc0]` means the vtable pointer is at `[rax]`, and the method pointer is at `[rax+0xc0]`. `rax` is loaded from `[r15]`. So the vtable is the one belonging to the object pointed to by `r15`.
     - Use `objdump` or `IDA` to find the vtable for likely classes in the binary. Search for cross‑references to known `IPlatformFile` subclass constructors. If we can find the vtable address, we can read the 25th slot (index 24) and jump to that code.
  4. **Static fallback**: Search the binary for functions that call `new` on a class whose name contains "Pak" or "PlatformFile". The constructor will store a vtable pointer. Find that vtable and extract slot 24.

### Move 2: Disassemble the non‑virtual function at `0x143b6fbf0` (Surface B)
- **Address**: `0x143b6fbf0`.
- **What to do**:
  1. Disassemble from that address for at least 50–100 instructions. Look for:
     - Calls to signature‑checking functions (e.g., `memcmp`, `FMemory::Memcmp`, `FRSA`, `SHA256` routines).
     - References to global variables that might hold a hash table or public key.
     - Any comparison of the pak file’s header fields (magic, version, etc.).
  2. If this function is short and simply returns success/fail, we can patch it to always return success after we pass A. That would be the exploit.

### Move 3: Search for SHA‑256 comparison patterns (Surface C)
- Even though we deferred C, we can run a quick parallel scan:
  - Look for byte patterns like `0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` (a sequence of 64 bytes that could be concatenated hashes). But better: search for `call` instructions that lead to `FSHA1` or `FSHA256` functions. UE provides `FSHA1::HashBuffer` and `FSHA256::HashBuffer`. Find those functions and list their callers. If any caller is in the neighbourhood of L3 or its called functions, that’s gold.
  - Also search for string `SHA` or `signature` in the code. Use `strings` and then `xref` to code.

---

## 4. Live Trace Recommendation (x64dbg)

**Primary goal**: capture the exact address of the vtable method that fails, and examine the state of the pak file object at that point.

### Breakpoint Setup
1. **Breakpoint at L3 entry**: `0x143b61be0`.
   - Watch: `rcx` (arg1, probably `this`), `rdx` (arg2, the r15 object), `r8` (arg3, the pak path string).
   - At entry, record the path. Step through to `0x143b61cac`.
2. **Breakpoint at `0x143b61cac`** (the vtable call).
   - At this point:
     - `r15` = arg2 (the object).
     - `rax` = `[r15]` (vtable pointer).
     - The call target is `[rax+0xc0]`. Read that address.
     - **Record**: vtable address, method address, and the current value of `rdi` (the `this` pointer of L3), especially `rdi+0x20` (the flag).
     - Run the call (step into) with a probe pak (which succeeds). Then run again with a production pak (which fails). **Note the method address** – if it is the same, the vtable entry is identical; the failure is due to checks inside that method. If different, the vtable may be different (custom subclass used for production paks?).
3. **Breakpoint at the failure block `0x143b61d24`**.
   - If the production pak fails, we land here. Capture the full register state and the path string.
4. **Breakpoint at the success path’s non‑virtual call `0x143b61d7a`** (which calls `0x143b6fbf0`).
   - Step into that call and trace the non‑virtual function.

### Watch Expressions and Memory Dumps
- At the vtable call: dump `r15` (object) first 0x200 bytes to see its fields – perhaps it stores the pak path, a hash, or a flags.
- After the vtable call returns, check `rax` for handle validity (non‑null if success). If null, examine the error condition inside the method.
- If possible, attach a second instance of x64dbg to a server that loads the probe pak and one that loads production, and compare the `rdi+0x20` flag. It might indicate whether encryption/signing is required.

### Expected Outputs
- Address of the vtable method for both probe and production (likely the same).
- Disassembly of that method (the first 100 instructions) – we can copy it to a file for later static analysis.
- The value of the flag `[rdi+0x20]` – if it differs between probe and production, that flag might control whether the vtable method enforces signature checks.

---

## 5. Decision Tree

Based on the results of the vtable method analysis (Surface A), the next actions branch as follows:

```
[Surface A] vtable method at slot 24 is identified.

|

+--- If method is standard `OpenRead`:
|    |
|    +--- Check its implementation: does it call any extra validation (e.g., file signature check)?
|    |    If YES: that validation is the signature check. Reverse it (Surface B+C combined).
|    |    If NO: then failure must be due to the file's content not matching some expected format (e.g., pak version, encryption flag). 
|    |         Compare headers of probe vs production pak. Likely a flag in the pak header tells SCUM to reject.
|    |         Solution: patch the header of the production pak or spoof the flag.
|    |
|    +--- If we cannot find the exact reason, the non‑virtual call (Surface B) becomes the next target.
|
+--- If method is a custom SCUM override (e.g., `SCUMPakPlatformFile::OpenPakWithSignatureCheck`):
|    |
|    +--- Disassemble it fully. Look for:
|    |    - Hash comparison against an internal allowlist (maybe stored in a static array or a file on disk).
|    |    - HMAC verification with a known key.
|    |    - Call to a function that reads a signature block from the pak itself.
|    |    Once the algorithm is known, the bypass is either:
|    |       a) Patch the function to always return a valid handle.
|    |       b) Add our pak's hash to the allowlist (if writable).
|    |       c) Spoof the signature inside the pak (if it's self‑contained like a normal pak signature).
|    |
|    +--- If the method calls into the non‑virtual function (Surface B), disassemble both together.
|
+--- If the method is unknown (we cannot find it in UE code):
|    |
|    +--- The vtable method may be a SCUM addition that is obfuscated. 
|    |    Use the live trace to extract its raw machine code, then decompile with capstone or Ghidra.
|    |    Look for any `.rdata` references (strings, constants) that hint at its purpose.
|
```

**Success for Surface A** is **identifying the exact reason why the vtable call fails for a production pak** – not necessarily fixing it yet. That reason could be:

- A flag in the object at `[rdi+0x20]` (e.g., “require signing” = true)
- A missing or invalid field in the production pak’s header (e.g., encryption flag not matching the key)
- A check inside the vtable method that compares the pak’s signature against a stored table and fails

If we can pinpoint that reason, we can move to a targeted bypass.

---

## 6. What Success Looks Like for the Next Session

The **shortest measurable progress** is:

1. **Identify the vtable method address** (via live trace or static analysis) and dump its first 100 instructions.
2. **Identify the flag or condition** that differs between probe and production paks at the vtable call site (e.g., the `[rdi+0x20]` flag, or a field in the pak header).
3. **Confirm whether the failure is inside the vtable method or in the non‑virtual function** – i.e., does the probe pak cause a return‑non‑null from the vtable call, and the production pak a NULL? If yes, we have localised the problem entirely to that method.
4. **At minimum, produce a clear one‑page summary** of the vtable method’s decompilation, including:
   - The function’s signature (what arguments it receives – likely the pak path, open flags, and the object).
   - The location of any comparison that leads to failure.
   - The string constants or data tables referenced.

This is achievable in 2–4 hours with the live trace and static disassembly. It does not require solving the full L3; it only requires **isolating the gatekeeper function**. Once we have its disassembly, the exploit path (patch, header spoof, or hash injection) becomes straightforward.

### Recommended Workflow for the Next 4 Hours

| Time | Activity |
|------|----------|
| 0:00–0:30 | Set up x64dbg, load SCUMServer.exe, attach to a server process, set breakpoints at L3 entry and vtable call. Use probe pak to capture success trace. |
| 0:30–1:00 | Run with a production pak (e.g., `GameData.pak`), capture failure trace. Record vtable address, method address, register states, and the flag `rdi+0x20`. |
| 1:00–2:00 | Static disassembly of the vtable method (or the non‑virtual call if the method is too complex). Use capstone+Python or IDA/Ghidra to decompile. |
| 2:00–3:00 | Identify the exact comparison/check that fails. Look for data table references, SHA256 calls, or HMAC routines. Compare the probe pak’s header with production – byte‑by‑byte diff. |
| 3:00–4:00 | Write up findings and propose the next exploit step (patch, header modification, or signature forge). Commit the iteration‑2 brief and add a iteration‑3 plan. |

If at any point the vtable method turns out to be simple (e.g., `OpenRead` that does no validation but production paks fail because of a flag in the object), the session can pivot to patching that flag or modifying the object before the call.

**Let’s isolate the gatekeeper.** That is the single most impactful next step.