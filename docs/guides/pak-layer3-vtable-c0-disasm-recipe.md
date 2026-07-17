# Pak Layer 3 vtable[0xc0] Disassembly Recipe
## Concrete Steps for UE 4.27.2 GameServer.exe

## Overview

You are investigating why the Pak Layer 3 (L3) function at VA `0x143b61be0` returns null for production paks but succeeds for a probe pak. The critical point is the call `call qword ptr [rax + 0xc0]` at `0x143b61cac`, where `rax = [r15]` (dereferencing r15 to get vtable).  
**Goal**: Identify which function vtable slot 24 (0xc0) resolves to, and why production paks cause it to return null.

This recipe assumes:
- You have a copy of `GameServer.exe` (x64 PE).
- You can use **PatternSleuth** for scanning and **Capstone** (Python) for disassembly.
- You have access to the probe-pak binary for testing (but not required for static analysis).

---

## 1. Find Callers of L3

L3 is at VA `0x143b61be0`. Use PatternSleuth to locate all direct `call` instructions targeting this address.

### PatternSleuth Invocation

```bash
patternsleuth scan --path GameServer.exe --xref 0x143b61be0 --summary -d
```

**Parameters explained**:
- `--path GameServer.exe` – target binary.
- `--xref 0x143b61be0` – find all instructions that refer to this address (default: calls, jumps).
- `--summary` – print a compact list of xrefs.
- `-d` – include disassembly context (shows a few lines around each xref).

**Expected output** (example):

```
xref(s) to address 0x143b61be0:
  0x143b61b00: call 0x143b61be0
  0x143b61d20: call 0x143b61be0
  0x143b61e50: call 0x143b61be0
...
```

You may get one or multiple callers. Choose **the first** (lowest address) as the most likely immediate entry point for L3 in the calling chain. If you only see one, that’s fine. If multiple, you may need to examine each; they likely correspond to different internal paths (e.g., for OpenRead vs OpenWrite?).

**Pro tip**: run with `-v` for verbose disassembly to see more context.

---

## 2. Disassemble the First Caller’s Preamble

We need to see how the caller prepares the second argument (`rdx`), which becomes `r15` inside L3.

### Python Capstone Script

Create a script `disasm_caller.py` that reads the binary, locates the caller, and disassembles its first ~30 instructions (enough to see argument setup). Below is a concrete template. You must adjust the base address if PatternSleuth reports offsets relative to image base (usually VA).  
Assume the image base for `GameServer.exe` is `0x140000000` (common for x64 PE). If your VAs are as given (0x143...), you don’t need adjustment; they are already absolute VAs.

```python
import capstone
import struct

# Configuration
BINARY_PATH = "GameServer.exe"
IMAGE_BASE = 0x140000000   # Change to match your binary's base (check PE header)
CALLER_VA = 0x143b61b00   # Replace with actual caller address from step 1
CALLER_DISASM_SIZE = 100   # bytes to disassemble from start of function

def read_bytes(filepath, offset, size):
    with open(filepath, "rb") as f:
        f.seek(offset)
        return f.read(size)

def file_offset_to_va(file_offset, image_base):
    # Simple mapping assuming flat binary; adjust if sections differ.
    return image_base + file_offset

def va_to_file_offset(va, image_base):
    # For a simple flat PE without relocation, assume Virtual Address equals file offset + base? No.
    # You must handle section alignment. Use a real parser (pefile) or manually map.
    # For simplicity, we'll fake it with a manual section table.
    # If you have IDA/Ghidra, export raw bytes at the VA.
    # If not, use `patternsleuth dump` to get bytes at the address.
    raise NotImplementedError("Use patternleuth dump or IDA to get raw bytes")
    return va - image_base   # may be wrong if sections are not at same offset.

# Since this is complex, we'll use an alternative: dump bytes via PatternSleuth.

# PatternSleuth: dump raw bytes from VA
import subprocess

def dump_bytes(va, size):
    cmd = f"patternsleuth dump --path {BINARY_PATH} --address {va:016x} --size {size}"
    result = subprocess.run(cmd, capture_output=True, text=True, shell=True)
    # Expect hex dump output; parse it
    # For a real script, you'd parse the output. Here we'll assume you put the bytes into a file.
    # Simpler: use IDA or Ghidra to disassemble and copy.
    raise NotImplementedError("Dump using PatternSleuth and parse, or use IDA")

# Instead, we present the script logic; you will adapt to your environment.
def disasm_caller():
    # Option 1: Read directly from binary if you can map VA to file offset.
    # Option 2: Use IDA Python to get bytes at caller_va.
    # Option 3: Use PatternSleuth to extract bytes in a hexdump, then convert to bytes.
    # We'll show Option 3 concept.

    # Use PatternSleuth to dump bytes into a file. Example manual step:
    # $ patternsleuth dump --path GameServer.exe --address 0x143b61b00 --size 0x100 -> output.bin
    # Then read output.bin.
    
    with open("caller_chunk.bin", "rb") as f:
        code = f.read()
    
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    # Starting address for disassembly (VA)
    start_va = CALLER_VA
    for insn in md.disasm(code, start_va):
        print(f"0x{insn.address:x}: {insn.mnemonic} {insn.op_str}")
        # Stop when we see the call to L3 (optional)
        if insn.mnemonic == "call" and CALLER_VA + insn.address - start_va ... # not trivial
```

**Better approach**: Use IDA or Ghidra’s disassembly, then copy the instructions or use their API.  
Since the user requested a **Capstone Python recipe**, we’ll give a script that works with IDAPython or a binary loaded by `pefile` to correctly convert VA to file offset.

### Full `pefile` + Capstone Script (If you have pefile)

```python
import pefile
import capstone

BINARY_PATH = "GameServer.exe"
CALLER_VA = 0x143b61b00   # replace
DISASM_LENGTH = 0x200

pe = pefile.PE(BINARY_PATH)
# For each section, find which contains CALLER_VA
for section in pe.sections:
    if section.contains_rva(CALLER_VA - pe.OPTIONAL_HEADER.ImageBase):
        # RVA = VA - ImageBase
        rva = CALLER_VA - pe.OPTIONAL_HEADER.ImageBase
        file_offset = section.PointerToRawData + (rva - section.VirtualAddress)
        break
else:
    raise ValueError("Address not in any section")

pe.close()
with open(BINARY_PATH, "rb") as f:
    f.seek(file_offset)
    raw_code = f.read(DISASM_LENGTH)

md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
print(f"Disassembly of caller at 0x{CALLER_VA:x}:")
for insn in md.disasm(raw_code, CALLER_VA):
    print(f"0x{insn.address:x}: {insn.mnemonic} {insn.op_str}")
    # Optionally stop after seeing the call instruction
    # if insn.mnemonic == "call" and "0x143b61be0" in insn.op_str:
    #     break
```

**Expected output** shows the caller’s function prologue and argument setup. Look for instructions:

```
mov rdx, ...   ; second argument setup
...
call 0x143b61be0
```

---

## 3. Identify r15’s Type from the Caller’s Disassembly

The second argument (rdx) becomes r15 in L3. In the caller’s code, find how rdx is set immediately before the call. Common patterns:

### Pattern A: `lea rdx, [global_addr]`  
This means r15 is a static global singleton. The address is typically a static pointer to a class instance (e.g., `&FPakPlatformFile::Singleton`?). Note the global address. Then check if that global is a known UE object (e.g., by scanning for its FName or nearby strings).

### Pattern B: `mov rdx, [this + offset]`  
If rdx comes from a member of `this` (rcx), then r15 is a member of the caller’s class. The offset tells you which member. For example, if the caller is FPakPlatformFile::OpenRead, the member at offset 0x? might be a sub-archive or writer. Cross-reference with UE 4.27 source for FPakPlatformFile fields (e.g., `InnerArchive`, `CachedFileSize`, etc.).  
- Common offsets: 0x0–0x100 depending on inheritance.

### Pattern C: `mov rdx, rax` after a function call  
The second argument came from a return value of another function. Disassemble that call too. The function might return `this` pointer after some initialization.

### Pattern D: `mov rdx, rcx` (rare but possible)  
Sometimes the second arg is the same as the first.

### What to look for in the disassembly:

- **Register uses**: Is rdx loaded from a constant (pattern A), from memory (pattern B), or from another register (pattern C).
- **The call to L3**: Often the caller has `call qword ptr [some_vtable + 0xN]` just before the L3 call? No, L3 is a direct function, not vtable call. So the caller is probably a wrapper.

Once you have the pattern, you can begin to narrow down the class.

---

## 4. If r15 is a UObject (UE Class)

If the value in rdx points to a UObject (class beginning with UObject vtable), the class can be identified via the `FName` stored at `UObject+0x18` (Field offset for `NamePrivate` in UE 4.27).

### Strategy to read FName at runtime (UE4SS Bridge)

In the UE4SS bridge handler that triggers on L3 entry:
1. Save the value of r15 (second arg). It is a pointer to an object.
2. Check if the object has a valid vtable (first qword points to a vtable within the module).
3. Read the `FName` at `obj + 0x18`. Parse the FName structure: first dword is index, second dword is number.
4. Call UE4SS Lua function `FindObject` with that FName to get the class name, or manually search the FName pool.

**Important**: r15 may not be a UObject; it could be a plain C++ class (FPakPlatformFile is not a UObject). But if it is, this method works.

### Static alternative: 
If you can identify the vtable address (from r15 dereference), in IDA you can search for references to that vtable and find the class’s constructor to pinpoint the UClass.

---

## 5. Locate the vtable[0xc0] Function

Once you identify the class of r15, find its vtable (likely embedded in the .rdata section).  
- In IDA: look for the vtable address. For example, if r15 is of type `FPakPlatformFile`, the vtable is `FPakPlatformFile::VTable`.  
- Slot 24 (`0xc0 / 8`) corresponds to the function pointer.  
- In UE 4.27.2 source, `IPlatformFile` vtable index 24 is **`OpenWrite`** (virtual function).  
- In SCUM, this may be overridden by a custom subclass.  
- Disassemble that function: it will be the target of the `call [rax + 0xc0]`.

**Static approach without knowing class**:  
If you can get the vtable pointer from r15 (e.g., in UE4SS bridge dump), then read the qword at vtable+0xc0, which is the address of the called function. Then disassemble that address.

**Alternative**: Use pattern matching: The call at 0x143b61cac is `call qword ptr [rax + 0xc0]`; the actual target address is not known statically because rax depends on r15. But you can set a breakpoint at that instruction with a debugger (like Cheat Engine or WinDbg) when the probe-pak succeeds, catch the target address, and then disassemble it statically.

---

## 6. Decision Tree

Based on findings from the caller disassembly:

```
Start: Identify how rdx is set in caller.

1. rdx = lea rdx, [global]
   -> r15 is global singleton. Check global's class by:
      - Checking if global is a known UE object (via static analysis or runtime dump).
      - Then locate its vtable.
      Proceed to step 5.

2. rdx = mov rdx, [rcx + offset]
   -> r15 is member of caller's class (rcx). 
   -> Determine caller's class by:
      - Looking at the caller's own vtable (if it is a UObject or has a vtable).
      - Find the offset's field name in UE source.
   -> Cross-reference: e.g., if caller is FPakPlatformFile and offset 0x? is "WritePakHandle", then r15 is a FPakHandle.
   Proceed to step 5.

3. rdx = mov rdx, rax after a function call
   -> Trace back the function call. The function likely returns a pointer to a created object.
   -> Disassemble that function to see what object it creates.
   Proceed to step 5.

4. rdx = something else (constant, immediate, etc.)
   -> Unlikely, but investigate further.
```

---

## Conclusion

By systematically applying this recipe, you will:

- Identify the static type / runtime type of the second argument (r15) to L3.
- Pinpoint the vtable slot 24 function (the target of the `call [rax + 0xc0]`).
- Then disassemble that function to understand why it fails for production paks but succeeds for the probe pak.

**Next steps after recipe**:  
1. Run PatternSleuth to get caller addresses.  
2. Disassemble the caller (using capstone + pefile or IDA).  
3. Analyze rdx setup pattern.  
4. Determine class.  
5. Locate vtable[0xc0] and disassemble the function.  
6. Compare behavior between probe and production paks (e.g., file path validation, encryption checks, format version).  

Once the function is identified, you can patch it or understand the condition that leads to returning null. Good luck.