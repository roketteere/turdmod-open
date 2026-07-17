# Path 3 Attack Plan – Crack SCUM’s Pak Layer 3 to Load Production UE 4.27 Pak Files  

**Goal**: Force `FPakPlatformFile::CreatePakHandle` (L3, VA `0x143b61be0`) to return a non‑null handle for any pak file, enabling arbitrary UE 4.27 content paks to be loaded by a running SCUM server. This plan avoids reliance on broken PolyHook2 and uses a mix of static analysis, safe memory reads, a minimal code patch, and a hand‑cooked validation pak.

---

## 1. Why PolyHook2 Failed and Alternative Hooking Strategies  

**PolyHook2 failure** at `0x143b61be0` likely due to:  
- `VirtualProtect` failure on the SCUMServer.exe code section (page protections not writable, or the section is `PAGE_EXECUTE_READ` only with no write allowed).  
- The prologue at `0x143b61be0` is too short for a 6‑byte JMP trampoline (only 5 bytes: `48 89 5c 24 08` = `mov [rsp+8], rbx`). PolyHook expects at least 5 bytes, but the allocation of the trampoline might have failed or the hook manager rejected it because of insufficient length for a full detour.  
- The code is in a `.text` section that is marked `MEM_IMAGE` with no write access; PolyHook tries to change it but might get `ERROR_NOACCESS` or similar.  

**Alternative strategies** (choose one, order of preference):  

### Strategy A: Manual JMP Patch with mprotect/VirtualProtect  
- We directly write a 14‑byte absolute JMP to a callback function in our injected DLL.  
- **Target offset**: `0x143b61be0` (L3 entry).  
- **Prologue backup**: first 14 bytes (to restore later if needed).  
- **Patch bytes**:  
  `FF 25 00 00 00 00` (6‑byte indirect JMP) followed by 8‑byte absolute address of callback.  
- **Protection change**: Use `VirtualProtect` with `PAGE_EXECUTE_READWRITE` on the page containing `0x143b61be0`.  
- **Implementation** in turdmod bridge (C++):  
  ```cpp
  DWORD oldProtect;
  VirtualProtect((LPVOID)0x143b61be0, 14, PAGE_EXECUTE_READWRITE, &oldProtect);
  BYTE patch[] = { 0xFF, 0x25, 0x00, 0x00, 0x00, 0x00 };
  memcpy((void*)0x143b61be0, patch, 6);
  *(uint64_t*)(0x143b61be0 + 6) = (uint64_t)L3HookCallback;
  VirtualProtect((LPVOID)0x143b61be0, 14, oldProtect, &oldProtect);
  FlushInstructionCache(GetCurrentProcess(), (LPCVOID)0x143b61be0, 14);
  ```  
- **Callback** must re‑implement the original logic but force success:  
  - Copy the original prologue to a trampoline (next to callback) and jump to `0x143b61be0+14` after handling.  
  - Inside callback, after the vtable[0xc0] call, if result is NULL, substitute with a dummy non‑zero pointer (see bypass section).  
- **Risk**: May be detected by anti‑tamper (SCUM has no known anti‑debug, but game server might have basic checks). We will patch only after the server has fully initialized (delay with a 5‑second timer after attach).  

### Strategy B: Hook a Function with a Longer Prologue  
- Instead of hooking L3 entry, hook a function that calls L3 **and** has a prologue ≥ 14 bytes.  
- The caller at `0x143b531a0` has a prologue: `48 89 5c 24 08 48 89 74 24 10 57 48 83 ec 20` (13 bytes). That’s enough for a 6‑byte JMP + 8‑byte pointer.  
- **Implementation**: same manual patch but at `0x143b531a0`. Inside the hook, we intercept the call to L3 and modify the return value.  
- **Advantage**: we don’t touch the critical L3 code; we intercept one level up.  

### Strategy C: Use MinHook (if available)  
- MinHook uses a different trampoline strategy (5‑byte relative JMP, then hook to a stub). It handles page protection automatically.  
- We can integrate MinHook into the bridge (single header library).  
- If PolyHook fails but MinHook works (often better with x64), we can hook `0x143b531a0` or `0x143b61be0`.  

**Recommended immediate action**: Use **Strategy A** (manual patch at L3 entry). It’s the most direct and requires no external library. The code is already prepared in the bridge (enableL3Probe handler) but we need to replace the PolyHook call with manual patch.

---

## 2. Pure Static Analysis to Identify r15’s Class (No Detour Needed)  

We need to know the class of the `this` pointer passed to L3 (stored in r15). The vtable[0xc0] call uses `[r15]` to get the vtable. The ultimate caller of L3 is at `0x143b67410` (or a similar address from prior research). We trace the chain:

1. **Locate the function that sets `rdx = [r13+8]`**  
   From previous analysis: at `0x143b67410` (let’s verify with capstone).  
   Open IDA or use patternsleuth:  
   ```
   patternsleuth execute --file SCUMServer.exe --pattern "48 8b 55 08 48 8b ce e8 ?? ?? ?? ?? 84 c0 75" --rva 0x143b67410
   ```
   This will capture the instruction `mov rdx, [r13+8]`.  
   If not exact, search for the sequence `48 8b 55 08 48 8b ce e8` (mov rdx,[rbp+8]? but previous doc said r13).  
   Actually we know from prior doc: “Caller-of-caller @ ~0x143b67410 sets rdx = [r13 + 8]”. We’ll use that RVA.

2. **Identify the class by finding the vtable**  
   In that function, r13 is the `this` pointer (set at entry). Follow the function’s prologue to see if it’s a method of a known class.  
   - The function likely belongs to `FPakPlatformFile` or a derived class.  
   - We can disassemble the function and look for references to global objects or string “Unable to create pak”. That string is in L3; the outer function might be `FPakPlatformFile::Initialize` or `CreatePakHandle` wrapper.  

   **Alternative**: scan the binary for all functions that contain the string reference to “Unable to create pak \"%s\" handle” (found at `.rdata` VA `0x14620cdb0`).  
   ```
   patternsleuth xref --file SCUMServer.exe --rdata-va 0x14620cdb0
   ```
   That gives the single reference in L3. Corroborate that L3 is the only consumer.

3. **Find the class of r13**  
   The caller-of-caller at `0x143b67410` is in the same compilation unit. We look for its vtable. Using capstone on that function, find where it stores a vtable pointer:  
   ```
   python -c "
   from capstone import *
   code = open('SCUMServer.exe','rb').read()
   md = Cs(CS_ARCH_X86, CS_MODE_64)
   start = 0x143b67410 - 0x140000000  (adjust base)
   end = start + 0x200
   for insn in md.disasm(code[start:end], start):
       if 'qword ptr [' in insn.op_str and 'r13' in insn.op_str:
           print(insn)
   "
   ```
   Look for `mov qword ptr [r13+xxx], offset` or `lea r13, [rip+...]`.  
   If the class is `FPakPlatformFile`, its vtable is at `0x14620A2B8` (from prior doc). We can verify: read the vtable at runtime and check if the first few entries point to known functions (like `OpenRead`, etc.). But we don’t need the class name for the bypass; we only need to override vtable[0xc0]. However, to ensure we patch the right thing, we must confirm that the vtable at runtime is indeed the one we think.

   Since we have runtime memory read (bridge handler), we can skip full static identification:  

---

## 3. Runtime vtable Read via Safe Bridge Handler  

No detour required. We already have an endpoint in the bridge that reads arbitrary memory (e.g., `turbo.read_memory(address, size)`). We can probe the vtable of the object passed to L3.  

**Procedure**:
1. **Find the address of r15** when L3 is called. We cannot directly access registers from C#/pipe. But we can set a breakpoint at runtime? No, we avoid breaking execution.  
   Instead, we hook L3 using our manual patch (Strategy A) and inside the hook, we capture `r15` (the this pointer) before the vtable call. Our callback receives the original arguments: `rcx=this`, `rdx=PakFilePath`, etc. The `this` pointer is in `rcx`. So `r15` is set from `rcx` at the top of L3.  
   In our hook callback, we can read the vtable pointer at `*(void**)rcx`. That vtable is `*(uintptr_t*)rcx`. We then read slot 0xc0 = offset `sizeof(void*)*0xc0` = `8*192 = 1536` bytes from that vtable.  
   We can send this information back to the bridge via shared memory or log.

2. **Create a small probe**: inside the hook callback, before the original code executes (or after we have intercepted), we log:
   - `this = rcx`  
   - `vtable = *(uint64_t*)rcx`  
   - `slot_c0 = *(uint64_t*)((uint64_t)vtable + 0xc0*8)`  
   - The function pointer at slot_c0.  

   Since the hook runs on every call to L3, we can trigger it by starting the server and then doing something that causes a pak load (e.g., connecting a client or triggering a level load). But easier: we can also call L3 ourselves with a dummy path (from the bridge) – but that requires a detour that returns properly. Instead, we rely on the server’s own pak loading when it reads `Paks/...`.

3. **Log output** to the bridge’s diagnostic window. We already have logging capability (e.g., `BridgeLog` or console).  

   **Risk**: premature hook before server initialization may cause crash. We will enable the hook only after server has finished loading paks (e.g., after `PostInit` event). We can detect server ready by watching for the string “Server started” in logs.

**Alternative**: If we want to avoid hooking entirely, we can use pattern scanning to find the slot_c0 function statically.  
- Read the vtable at `.rdata` VA `0x14620A2B8`.  
- Get the 0xc0th entry: `vtable[0xc0]`. Since vtable is an array of function pointers, the entry at index 0xc0 is at offset `0xA2B8 + 8*0xC0 = 0xA2B8 + 0x600 = 0xA8B8` in .rdata.  
- Disassemble that function (VA = read pointer from `.rdata+0xA8B8`). That function is the validator that returns null for production paks.  
- That matches the function we analyzed in `docs/guides/pak-layer3-fn-3b6fbf0.md`. So we already know the validator is at `0x143b6fbf0`.  
- We can confirm by checking that the function at `0x143b6fbf0` contains the signature check logic (likely calls `CheckPakSignature`).  

**Thus we already have the target: function at VA `0x143b6fbf0` (the vtable[0xc0] function).** No runtime read needed, but we can verify.

---

## 4. Bypass Strategy – Simplest Patch to Always Return Non‑Null  

The vtable[0xc0] function (VA `0x143b6fbf0`) returns a pointer to a `FPakHandle` or null. Our production paks cause it to return null. We want to force it to return a non‑null (valid handle).  

**Observation**: The function likely checks the pak file’s signature or some embedded data. For “probe” paks (e.g., the official SCUM paks), it returns a valid handler. For our custom paks, it returns null.  

**Simplest patch**: In that function, at the very beginning, `xor eax,eax; ret` would return null. We want the opposite: overwrite the function to `mov rax, <some_handle>; ret`. But we don’t know a valid handle to return. However, the caller (L3) after receiving a null handle will log an error and return null; if we change the vtable[0xc0] to return a non‑null but invalid pointer, the subsequent code may crash when trying to use it.  

**Better approach**: Patch the function to call the original function but if it returns null, return a **dummy placeholder** that L3 will treat as a valid handle but that later usage will gracefully handle (or never be used before freeing). Alternatively, patch L3 itself to ignore a null from vtable[0xc0] and proceed with a default behavior.  

But from earlier research (docs), L3 after vtable[0xc0] call checks if rax is null and jumps to error block. If we can change that check to always treat rax as non‑null, we bypass.  

**Thus the simplest patch is at L3 itself, not at the validator.**  

- At L3 address `0x143b61cac`, the instruction after the call is `test rax, rax; je <error_block>`. We can NOP the `je` or change it to `jmp` to always skip error.  
- This will make L3 continue with the returned value (even if null). That might cause a crash later when the handle is dereferenced, but we can also ensure that the handle pointer is replaced with a valid fake object.  

**Better**: In our hook callback (Strategy A), after the original L3 returns, we can check the return value (`rax`) and if it’s null, substitute it with a **pointer to a global dummy `FPakHandle`** that we allocate in our DLL. This dummy handle must mimic the minimal interface expected by the caller (probably just some vtable with empty functions). We can create a minimal class in our DLL that inherits from `FPakHandle` (or whatever the base is) and has a single vtable entry that does nothing.  

**Implementation**:  
1. In turdmod bridge, define a struct `FakePakHandle` with a vtable pointer pointing to a static vtable with one entry (the dummy function).  
2. In the L3 hook callback, after the original function returns, if `result == NULL`, set `result = &g_FakePakHandle`.  
3. The hook must intercept at the right point: either after the vtable[0xc0] call (inside L3) or at the end of L3.  

Given the complexity, the **simplest and safest patch** is to change one byte in L3: at offset `0x143b61cb1` (the conditional jump after `test rax,rax`), change `74 ??` (je) to `EB ??` (jmp) to always skip the error block. But we must compute the offset correctly.  

**Exact byte patch**:  
- Instruction at `0x143b61cac`: `48 85 c0 74 14` (test rax,rax; je +0x14).  
- Change `74 14` to `EB 14` (jmp +0x14). This makes it always jump to the success path, regardless of rax.  
- This patch requires no memory allocation, no dummy objects. The code after the jump will proceed to construct a `FPakHandle` from the return value? Actually after the error block, L3 returns null; after the jump we go to the path that assumes rax is valid. That path will likely try to dereference rax (e.g., `mov rcx, [rax+0x10]`). If rax is null, it will crash. So we must ensure that rax is not null when we jump. But if rax is null, the jump will cause a null dereference.  

**Alternative**: Instead of altering the conditional, we can modify the call to vtable[0xc0] to instead call our own function that returns a valid fake handle. That’s a more invasive patch but cleaner.  

- Patch the call instruction at `0x143b61cac`: `FF 14 8F` (call qword ptr [rdi+rcx*4]) to `E8 ...` (call our function). But that’s a 5-byte relative call. We need to know the offset to our callback.  
- Simpler: hook vtable[0xc0] function itself (VA `0x143b6fbf0`) with a manual patch that forces it to always return a non‑null pointer. That function’s return is a handle, which is a pointer. If we make it always return `&someValidStaticObject`, the subsequent code will treat it as a real handle and may actually work (if the handle is never actually used – just stored and freed later). We can create a dummy `FPakHandle` in our DLL.  

**Final verdict**: The best bypass is to **hook the vtable[0xc0] function** (0x143b6fbf0) and force it to return a pointer to a global dummy `FPakHandle` object that we allocate. This avoids altering L3 logic and minimizes side effects.  

**Steps**:
1. Define a struct that matches `FPakHandle` (size and vtable pointer). In UE 4.27, `FPakHandle` is probably `class FPakHandle` with virtual destructor and maybe `Seek`, `Read`, etc. We can create a minimal fake with a vtable pointing to a single function (e.g., virtual destructor that does nothing).  
2. In our hook callback, we simply return the address of this static instance.  
3. The original function is never called.  
4. L3 will receive a non‑null handle and treat it as valid.  

**Risk**: If FPakHandle’s vtable is used for Seek/Read calls later, those will dispatch to our dummy functions, which may crash if they don’t handle arguments. But we can implement stubs that just return success (e.g., `Read` returns 0 bytes, `Seek` returns true). We know the exact vtable layout from UE source (we have UE 4.27 installed).  

**Implementation details**:  
- We need to know the size of `FPakHandle`. From UE 4.27 source: `class FPakHandle : public FRunnable` (or similar). Actually check `Engine/Source/Runtime/PakFile/Public/IPlatformFilePak.h`. The handle is `struct FPakHandle`? I recall `FPakFile` contains the handle. But we can just allocate a buffer of, say, 256 bytes and use it as a dummy handle.  
- The vtable pointer at offset 0 must point to our custom vtable. The vtable must have at least as many slots as the real one (at least up to destructor). We can copy the real vtable from a valid pak handle (if we can get one) and replace only the signature check function. But that’s complex.  

**Simplest immediate bypass**: Instead of returning a fake handle, we can modify the byte in L3 that checks the return value. As noted, that may crash. But we can combine with a second patch: also return a valid handle from our hook of vtable[0xc0]. Let’s do the vtable[0xc0] hook approach, even if it requires constructing a minimal fake.

**Construction of fake FPakHandle**:  
- Read the memory of a real FPakHandle after a successful load (e.g., while server is running, read one of the official pak handles). We can find one by scanning GUObjectArray for `FPakFile` objects? That’s overkill.  
- Instead, allocate 0x100 bytes, set first 8 bytes to point to our own static vtable (which we craft).  
- Our static vtable will have entries that point to existing functions that are safe to call (e.g., `UE4::FArchive::Serialize` that does nothing). We can hijack the vtable from an existing object that is known to work (like `FArchive`).  

**Given time constraints, let’s choose the simplest path**: patch the conditional jump in L3 and accept the risk of crash. If crash occurs, we revert within 3 seconds. We can test with a non‑critical pak.

---

## 5. Smallest Test Pak Creation – UE 4.27 Content Pak with Single Blueprint Function  

We need a pak file that, when loaded, registers a Blueprint Function Library in GUObjectArray that we can call from the bridge to verify success.  

**Steps** in UE Editor (4.27.2):  
1. Create a new Blueprint Function Library class (C++ or BP). For simplicity, create a Blueprint Function Library (pure BP).  
   - Name: `BPFL_Test`  
   - Create a Blueprint Callable static function: `static void TestFunc()` that logs to the output log.  
2. Package this as a content pak:  
   - In UE Editor, go to File → Package Project → Windows (64-bit) → choose output folder. This creates a standalone executable. Not what we want. Instead, we want only the cooked assets.  
   - Use the `UnrealPak.exe` command line to create a pak from a cooked directory.  
   - Better: follow the official method:  
     1. Cook the content using `UE4Editor-Cmd.exe <project> -run=cook -targetplatform=WindowsNoEditor -cookonthefly -unversioned`.  
     2. Then use `UnrealPak.exe <output.pak> -create=<input.txt>` where input.txt lists files.  
   But we don’t have the full project. Simpler: create a minimal standalone test by cooking a single asset and then packing.

   Given we are not inside UE Editor, we can use a pre‑cooked asset from a known game? Not recommended.  

   **Alternative**: Use the existing SCUM paks as base? No, they are encrypted/signed.  

   **Simplest test**: Use a pak that is known to work on a vanilla UE 4.27 server (like a simple map). But we don’t have a server setup.  

   **Instead, we can test our patch by trying to load a pak file that SCUM itself has in its `Paks` directory but maybe is not loaded due to signature. For example, `pakchunk2_s1-WindowsNoEditor.pak` might be a production pak that gets rejected. We can force it to load and see if the server crashes or functions. That would be a real test.

   **Recommendation**: Use an existing SCUM pak that is currently NOT loaded by the server (e.g., one of the optional DLC chunks). We can try to load it via the bridge after our patch.

   **If we still want a custom pak**:  
   - Create a minimal directory structure: `MyGame/Content/MyFolder/MyAsset.uasset` and `MyAsset.uexp`.  
   - The asset can be a simple Blueprint that inherits from something.  
   - Use `UnrealPak.exe MyPak.pak -create=FileList.txt -platform=WindowsNoEditor -compressed`.  
   - We can create a dummy uasset using a hex template? Too heavy.  

   **Therefore, skip custom test pak for now; rely on existing SCUM paks that fail.**

---

## 6. End-to-End Test Recipe  

1. **Apply patch** (hook vtable[0xc0] function or L3 conditional jump). For initial test, we use the byte patch on the conditional jump to always skip error (despite risk).  
2. **Start server** with the patch active (enable the hook in `enableL3Probe` after server ready).  
3. **Trigger a pak load** that would previously fail. E.g., request the server to mount a production pak via console command or by dropping a new pak into `Paks` folder and calling `reloadpaks` (if such command exists). SCUM may not have such command; but we can force a pak mount by using the bridge to call `FPakPlatformFile::Mount` directly? That requires knowing the signature. Instead, we can simulate by manipulating the directory watch (if any).  
4. **Monitor logs**: look for the string “Unable to create pak ... handle” – if not printed, the patch succeeded.  
5. **Verify asset registration**: after pak is mounted, we can scan GUObjectArray for any new objects (e.g., a Blueprint class from the pak). We have the bridge’s ability to iterate GUObjectArray (`GUObjectArray` pointer known). We can check if the number of objects increased or if a specific class appears.  
6. **If crash occurs**: auto‑revert within 3 seconds (the bridge has a watchdog that reverts patches if server dies).  

**Success indicator**: No “Unable to create pak” log, server stays up, and new UE objects appear in GUObjectArray.

---

## 7. Risk Mitigation  

- **Patch applied too early**: server may crash during initial pak loading (e.g., engine startup loads many paks). We delay activation until after server has processed its initial pak set (e.g., after `WorldPreInit` or after first `PostLogin`). We can set a timer: activate patch 10 seconds after DLL load.  
- **Incorrect patch offset**: double‑check with disassembly. Use capstone to verify the byte at `0x143b61cb1` is indeed `74 14`.  
- **Server crash even after delay**: the patch may cause a null pointer dereference downstream. The auto‑revert will remove the patch, allowing server to recover. We can also catch the exception with `__try/__except` around the patch? Not feasible for code execution patches.  
- **Anti‑cheat detection**: SCUM server has no known anti‑cheat, but common EAC is not present on dedicated server. No issue.  

**Recovery plan**: The autonomous deploy system (mentioned) can revert to previous DLL within 3 seconds if server exits. We set a watchdog thread that monitors server process and reverts on exit.

---

**Execution priority**:  
1. Implement manual JMP hook at vtable[0xc0] function (0x143b6fbf0) returning a fake handle (allocate buffer, point to dummy vtable).  
2. Alternatively, patch the conditional jump in L3 if hook is too complex.  
3. Test with an existing SCUM production pak (e.g., `pakchunk2_s1-WindowsNoEditor.pak`).  
4. If success, proceed to load custom content pak.  
5. Document findings and escalate to Joel.

Let’s go.