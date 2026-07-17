# SCUM Pak Layer 3 (L3) Research Brief

## 1. Map of L3 – RVA 0x03b61be0

### Likely UE Source Context

Based on UE 4.27.2 `FPakPlatformFile` and `FPakFile` source (Engine/Source/Runtime/PakFile/Public/IPlatformFilePak.h, Private/PakFile.cpp), the most probable match for a function that returns a pak handle (`FPakFile*` or `TSharedPtr<FPakFile, ESPMode::ThreadSafe>`) after performing structural/signature checks is:

- **`FPakPlatformFile::ReadPakFile`** (internal UE function, not part of the public API but present in the UE source) or
- **`FPakFile::TryLoadSignature`** followed by structural validation, then **`FPakFile::Initialize`**.
- Another candidate: **`FPakFile::Open`** (static factory) which calls `TryLoadSignature` and then creates the handle.

The calling context is likely the main pak mount point in `FPakPlatformFile::Mount` or `FPakPlatformFile::FindPakFile`. The function at 0x03b61be0 receives a pak filename string, attempts to open the pak, reads its header, verifies the embedded signature (if any) against an embedded public key, and returns a valid handle or null.

### Why Return-Null at L3?

The probe-pak (HelloWorld) works because its pak header fields (e.g., mount type, encryption flag, chunk size) happen to satisfy the validation path – perhaps it’s an unencrypted, small pak. Production paks likely contain:

- Encrypted index or chunk data (detected by header flags → triggers signature validation)
- A different mount type (e.g., `PakChunk` instead of `PakSingle`)
- A mismatched `FPluginDescriptor` or asset registry structure that SCUM checks against a whitelist.

### Function Shape (Expected from UE open source)

```
bool FPakPlatformFile::OpenPakFile(const FString& PakPath, bool bWaitForKey, TSharedPtr<FPakFile, ESPMode::ThreadSafe>& OutPakFile)
```

or a similar signature. The function:

1. Opens the `.pak` file (and optionally `.sig` sidecar).
2. Reads the pak header (magic, version, index offset, etc.).
3. Checks for signature: if `HasSignature()` flag is set, loads `.sig` file or embedded signature, validates against embedded RSA public key.
4. If validation passes or not required, creates a `FPakFile` object and initializes it.
5. Returns true (handle valid) or false (returns null in parameter).

The return-null at L3 suggests signature validation fails or structural integrity check (e.g., block count, chunk table sanity) fails for production paks.

---

## 2. Specific Sigscan Moves (Ranked by ROI)

### Move #1: Disassemble L3 – Immediate xref and disassembly

**Command:**
```bash
patternsleuth xref --rva 0x03b61be0 -e GameServer.exe -d 200
```
**What to look for:**

- Check if the function is large (> 100 instructions) – indicates complex logic.
- Identify `call` instructions to other functions. Note their RVAs – they are subroutines (e.g., `FPakFile::TryLoadSignature`, `CheckPakSignature`, `ReadPakHeader`, `VerifyPakIntegrity`).
- Look for comparisons against constants specific to UE pak format:
  - `cmp eax, 0x5343414D` ("PACK" magic) – likely immediate.
  - `cmp dword ptr [rdi+0x18], 11` (or similar) – pak version check.
  - `test byte ptr [rcx+0x4C], 0x40` – signature flag.
- Look for conditional jumps leading to a `xor eax, eax` (return 0/null) path. That is exactly the fail path we need to understand.

**Immediate value:** Determine exactly which condition fails: whether it’s a signature check, version check, or structural defect. This tells us if we need a forged `.sig` file or a pak header patch.

### Move #2: Find the embedded public key RSA blob

UE stores the RSA public key as a binary blob (DER/PKCS#1) inside the executable. The blob is typically loaded via `FRSA::PemToDer` or a hardcoded byte array. The pattern: starting with `0x30 0x82` (SEQUENCE, length can be up to 0xFFFF) followed by `0x02 0x82` (INTEGER, modulus length ~256/512 bytes for 2048/4096-bit keys) and `0x02 0x03` (exponent = 3 or 0x10001).

**Command:**
```bash
patternsleuth find --bytes "30 82 ? ? 02 82 ? ? 02 82 ? ? 02 03 01 00 01" --wildcard --search-data --file GameServer.exe
```

**If first pattern fails** (exponent typically 0x010001, but could be 3), try:
```
30 82 ? ? 02 82 ? ? 02 03
```

**What to do with found blob:**
- Record offset (RVA) and length (first 2 bytes after 30 82 tell you total remaining length).
- Dump the raw bytes (including the SEQUENCE header) to a file: `pubkey.der`.
- Use `openssl pkey -inform DER -pubin -in pubkey.der -text -noout` to get modulus and exponent.

**Immediate value:** Once we have the public key, we can decide:
- Is the key weak? (e.g., 512-bit? unlikely but check)
- Can we sign a pak with a matching private key? (only if we can extract/derive, or patch with our own)
- Does the key match the default UE test key? (default UE ships with a well-known RSA key; if SCUM didn't replace it, we already have the private key!!! – see `Engine\Source\Runtime\PakFile\Private\PakFileUtilities.cpp` for the test key `TestPrivateKey` / `TestPublicKey`.)

### Move #3: Trace L3 calls from x64dbg with a production pak

Live attach to GameServer.exe when it fails on boot. Set breakpoint at 0x03b61be0. Run until break. Use `bp 0x03b61be0`. On break:

- Look at the first argument (RCX in Windows x64 calling convention). It should be a `FString` pointer (`TArray<wchar_t>` with length and data). Dump the string to confirm it's our production pak path.
- Step over (F10) and observe registers after each `call` instruction – look for return values (RAX = null/non-null). When a subcall returns 0, that’s likely the failing validation.
- Use `r d` to see if RAX holds something like a pointer to a `FPakFile` object at the end. If RAX ends up 0, we know the function returns false.

**Immediate value:** Pinpoint which subroutine inside L3 kills the handle creation.

### Move #4: Find all callers of L3

**Command:**
```bash
patternsleuth xref --target-rva 0x03b61be0 -e GameServer.exe
```
**Expected:** At least one caller in the mount path (e.g., `FPakPlatformFile::MountAllPakFiles`). Possibly two: one for initial mount and another for dynamic pak loading (mod support?). If SCUM has a custom pak loader, there may be fewer callers.

Check the caller’s logic: Does it retry? Does it log an error? Knowing the caller helps design a hook that returns a valid handle without triggering side effects.

---

## 3. Three Candidate Exploitation Paths

### Path A: Hook L3 to Always Return Non-Null with a Fake Handle That Bypasses Precacher Race

**Effort:** High (2–3 sessions) | **Risk:** High | **Likelihood of success:** Medium

**Idea:** Force `return true` at L3 but also lie about the `OutPakFile` parameter by providing a pointer to a pre-constructed `FPakFile` object. The downstream code dereferences the handle to access the pak's file table, mount path, and status flags.

**Challenges:**
- The `FPakFile` object is nontrivial. We must craft one that the precacher doesn't crash on. The minimal object size is ~0x350 bytes (UE 4.27: `FPakFile` has a `FArchive`, `TPakFileSerializer`, `FSignatureData`, etc.).
- We could allocate a "dummy" `FPakFile` by duplicating the working `FPakFile` created for the probe-pak. That is possible if L3 is called per pak (it is). We could:
  - Hook L3 to return the existing working pak's handle for our production paks. But that would confuse the pak file system (two paks with same handle) – likely crash later.
- Better: Create a legitimate `FPakFile` by calling into the original L3 but patching its return value from false to true and also providing a valid handle. However, if L3 returns false because signature validation fails, the function probably cleans up partial allocations, so we can't steal its handle.

**Alternative sub-path (lower risk):** Hook only the beginning of L3 to bypass the signature check but continue to the structural validation (which the probe-pak passes). That means we intercept at the point where the signature flag is tested and force it to think there is no signature.

**How to implement:** Find the `test byte, 0x40` instruction (or similar) and patch it to `mov byte ptr [rsp+0x78], 0` (clear flag) just before the call to `TryLoadSignature`. This effectively makes the pak appear unsigned. Then the function proceeds with normal initialization.

**Risk:** If the production pak’s index is encrypted (UE supports encrypted pak indices when signing is used), disabling signature check will prevent decryption, leading to garbled index and crash during loading. We would need to also handle decryption key injection. But typically encrypted paks require a symmetric key set via `PakEncryptionKey` – SCUM might use the default test key, or a custom one we can find in the exe (similar to the public key search but for AES).

### Path B: Generate a Structurally-Valid `.sig` Sidecar File

**Effort:** Medium (1–2 sessions) | **Risk:** Low | **Likelihood of success:** Medium-High

**Idea:** Create a `.sig` file that tells L3 the signature is valid. There are two variants:

1. **If SCUM uses the default UE test key:** We already have the private key (`TestPrivateKey` in UE source). We can sign any pak using `UnrealPak.exe -Create -Sign` with our own certificate/key. Actually, UE’s `SigCreator` tool can produce a `.sig` file for a given pak. We just need the exact same public key embedded in GameServer.exe. If it’s the default test key, we are done – just sign our pak with the matching private key.

   **Check:** After extracting the embedded pub key (Move #2), compare modulus to the known default test key modulus (`DA 35 55 7A ...` / `A3 3B F2 ...`). If match, bingo.

2. **If SCUM replaced the key:** We need to modify the exe to accept our own public key (see Path C) or find a way to bypass the signature verification without sidecar.

**How to implement (if default key):**
```powershell
.\UnrealPak.exe MyProductionPak.pak -Create=MyPakInput.txt -Sign=c:\path\to\privatekey.pem
```
This produces `MyProductionPak.sig`. Place it next to the pak in the `Content/Paks` folder. SCUM’s L3 should find it and validate against the embedded pub key.

**Test:** Use the probe-pak first: sign it, rename the sig to match the probe-pak, and see if it still loads (it should, with or without signature). If it fails, the embedded key is not the default.

**Risk:** Low; if it doesn’t work, we only waste a few minutes.

### Path C: Patch the Embedded Public Key in Memory

**Effort:** Medium (1–2 sessions) | **Risk:** Medium | **Likelihood of success:** High (if we can find the key data)

**Idea:** Replace the RSA public key blob in GameServer.exe with our own public key (for which we have the private key). Then sign our paks with our private key.

**Steps:**
1. Extract the original public key blob (Move #2). Note its RVA and size.
2. Generate a new RSA-2048 key pair: `openssl genpkey -algorithm RSA -out private.pem -pkeyopt rsa_keygen_bits:2048`
3. Extract public key DER: `openssl pkey -in private.pem -pubout -outform DER -out pubkey.der`
4. The new DER blob may be same size or different. If same size, simple memory patch. If different, we need to be careful not to overwrite adjacent data. Usually UE stores it in a static byte array with fixed size (e.g., 294 bytes for 2048-bit key). We can generate a key of same size (matching length), or pad/truncate.
5. Write a hook (DLL) that at process startup (via `DllMain` or a trampoline in `WinMain`) patches the binary memory (e.g., using `VirtualProtect` and `memcpy`).
6. Sign our paks with our private key.

**Risk:** The patched key must be placed at the exact address where the code reads it. If the code computes the address of the blob relative to the module base (e.g., `lea rcx, [rip+offset_to_pubkey]`), patching that pointer would require a two-step hook. But since UE typically references it as a global variable, direct memory patching works.

**Test:** Create a tiny test pak, sign with our key, load on patched server. If it works, we have a permanent solution.

**Alternative – no patch needed:** If we can sign with the original private key (extracted via vulnerability or if key is weak), we could avoid patching. Unlikely.

---

## 4. Risk Assessment Per Path

| Path | Server Crash Risk | Fingerprintability | Effort | Recommendation |
|------|-------------------|-------------------|--------|----------------|
| **A: Hook L3 to fake handle** | High – fragile assumptions about precacher state | Medium – must maintain hook across updates | High | Avoid initial |
| **A sub: NOP signature flag** | Medium – may cause decryption failure for encrypted paks | Low – simple patch | Medium | Try if Path B fails and paks are unencrypted |
| **B: .sig sidecar** | Low (sidecar is official UE mechanism) | Low – indistinguishable from legit mod | Low | **Do first** |
| **C: Patch pub key** | Low – stable memory patch | Low – patched exe looks modified but runtime no extra issues | Medium | Fallback after B fails |

**Conclusion:** Path B (check default key) is the highest ROI and lowest risk. Execute it before any other.

---

## 5. The Next Concrete Move (30 Minute Execution)

**Action:** Verify if SCUM uses the default UE test key.

### Step-by-step:

1. Locate the embedded public key blob.
   ```bash
   patternsleuth find --bytes "30 82 01 0A 02 82 01 01 00" --search-data --file GameServer.exe --output-rva
   ```
   (The pattern `30 82 01 0A` is typical for a 2048-bit RSA public key DER: SEQUENCE length 0x010A, followed by INTEGER length 0x0101). The exact pattern may vary – adjust length bytes as needed (0x010A for 2048-bit, 0x0202 for 4096-bit). Use wildcards: `30 82 ? ? 02 82 ? ?`.

2. Once found, dump the blob (e.g., RVA 0x14D3020, size 290 bytes). Save as `scum_pubkey.der`.

3. Run:
   ```bash
   openssl pkey -inform DER -pubin -in scum_pubkey.der -text -noout
   ```
   If the output shows a modulus and exponent, we have the key.

4. Compare the modulus to the default UE test public key (available in UE source, `Engine\Source\Runtime\PakFile\Private\PakFileUtilities.cpp`, variable `TestPublicKey`). You can compute SHA256 of the DER blob and compare.

5. **If match:** We’re golden. Sign a test production pak with the default private key:
   - Obtain private key: Search UE source for `TestPrivateKey` (PEM string) or use a known default DER. You can also export from a clean UE 4.27.2 installation by creating a test pak with `-Sign` and then extracting the private key from the engine binary (or just use the string in the source).
   - Sign our production pak (e.g., `BP_TurdMODQuartermaster_P.pak`) using UnrealPak.exe:
     ```powershell
     .\UnrealPak.exe .\BP_TurdMODQuartermaster_P.pak -Sign="PathToDefaultPrivateKey.pem"
     ```
   - Place the resulting `.sig` file in the same Paks folder.
   - Boot the server. If it loads without crash, L3 is bypassed.

6. **If mismatch:** We still have the public key. Next session we attempt Path C (patch the key) or investigate the signature flag bypass (Path A sub).

**Time estimate:** 20–30 minutes to execute steps 1–5.

---

## 6. Open Questions to Be Answered by Next Session

1. **Is the embedded public key the default UE test key?** (Answered by the comparison above.)
2. **Does L3 even check the signature when no `.sig` file is present?** Probe-pak works without a `.sig` – so it might skip signature validation entirely for some paks. Which property of the probe-pak triggers that skip?
   - We can test by adding a `.sig` to a probe-pak that we sign incorrectly. If it still loads, signature check is bypassed.
3. **Are production paks flagged with encryption?** We can check the pak header byte at offset 0x10 (flags). The probe-pak has flags=0x0? (look at hex dump). Production paks may have `HAS_SIGNATURE` (0x40) or `HAS_ENCRYPTED_INDEX` (0x02) flags set. These flags are set by UE during cooking if the project configures signing. SCUM likely has `bEncryptIndex=false` but `bGeneratePakSignature=true` in `Game.ini`.
4. **Can we disable the signature flag in the pak header at runtime using a hook?** That would allow production paks to pass without a signature. This is essentially Path A sub, but more robust than NOPing the code, because we change the input (the pak file memory) to appear unsigned.
5. **What is the function at L3's callee (e.g., `0x03B62500`) that actually fails?** We need to disassemble it and find the error string it uses (if any). The string "PakFile signature mismatch" is already suppressed by v4 hook, but there may be other error messages like "Failed to load pak file signature."

---

## Summary of Next Session Plan

| Step | Action | Output | Time |
|------|--------|--------|------|
| 1 | Find embedded pub key blob (patternsleuth search) | RVA and DER file | 5 min |
| 2 | Compare to default UE test key (SHA256) | Same/Mismatch | 2 min |
| 3 | If same: sign production pak with default private key, deploy .sig | Load test | 10 min |
| 4 | If different: disassemble L3 (Move #1) and identify signature check routine | Understanding of fail path | 15 min |
| 5 | Based on results, pick exploitation Path (B/C) | Decision | 5 min |

This plan guarantees that within 30 minutes we either bypass L3 (if default key used) or have a clear map of L3 internals to design a custom hook.

**Let’s execute.**