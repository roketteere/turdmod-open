# UE 4.27 Alternative Pak Signature Scheme Inventory for SCUM

## Overview

SCUM (UE 4.27.2) does **not** use standard PKCS#1 RSA for pak signature validation.  
No DER-encoded SubjectPublicKeyInfo or RSAPublicKey SEQUENCE found in `SCUMServer.exe`.  
Game uses a bypass (turdmod commits) for v3.1/v4/v5 paks, but production paks still fail at layer 3  
(`Unable to create pak "%s" handle`, RVA `0x03b61be0`). This suggests SCUM relies on a **non-RSA** or **custom** validation scheme.

Below is an inventory of possible UE 4.27 alternative modes, how to detect each, and a scanning priority.

---

## 1. FSHA1 Chunk Signing (UE3‑era method)

| Property          | Detail                                   |
|-------------------|------------------------------------------|
| **UE source path** | `Runtime/PakFile/Private/PakFile.cpp` (`FPakPlatformFile::CheckPakSignature`) |
| **How it works**  | Pak contains a SHA1 hash per data chunk. Verifier recomputes hash and compares. |
| **Detection**     | Look for constant string `"SHA1"` (ASCII) in `.rdata`. Also look for SHA1 context initialization bytes (e.g., `0x67452301`, `0xEFCDAB89`, etc.) |
| **Patternsleuth** | `patternsleuth -f SCUMServer.exe -s "SHA1"` |
| **Remarks**       | Rare in UE 4.27; mainly kept for backward compat. SCUM would likely not use this alone because it provides no signing – only tamper detection. |

## 2. HMAC-SHA256 with Embedded Symmetric Key

| Property          | Detail                                   |
|-------------------|------------------------------------------|
| **UE source path** | `Runtime/PakFile/Private/PakFile.cpp` (custom extension) |
| **How it works**  | Pak hash is signed with HMAC-SHA256 using a symmetric key embedded in the executable. Verifier derives HMAC and compares. |
| **Detection**     | Search for HMAC initialization constants: `0x36` repeated 64 times (ipad) and `0x5c` repeated 64 times (opad). Also look for a 32‑byte key stored as literal bytes (no string reference). |
| **Patternsleuth** | `patternsleuth -f SCUMServer.exe -s "$hex:36,64"` for ipad; similar for opad<br/>Then scan for `$hex:??` blocks of size 32 (AES‑256 key). |
| **Remarks**       | Moderate probability. UE 4.27 does not ship this natively, but many custom anti‑tamper solutions use HMAC‑SHA256. |

## 3. Binary AES Key Comparison

| Property          | Detail                                   |
|-------------------|------------------------------------------|
| **UE source path** | `Runtime/PakFile/Private/PakFile.cpp` (if AES encryption is used for pak data) |
| **How it works**  | The executable contains a plaintext AES‑256 key. During mount, the key is compared against an embedded copy. If match, pak is trusted. |
| **Detection**     | Search for a continuous block of 32 bytes that appear only once in `.rdata` and have high entropy (likely an AES key). Scan for `$bytes:32` with entropy > 7.0. |
| **Patternsleuth** | `patternsleuth -f SCUMServer.exe --entropy-min 7.0 --size 32 .rdata` |
| **Remarks**       | Detection is noisy; key could be XOR‑obfuscated. However, SCUM uses AES for pak data (common in UE4). The signature key may be stored **separately** from the data encryption key. |

## 4. CRC32/CRC64 with Embedded Checksum Table

| Property          | Detail                                   |
|-------------------|------------------------------------------|
| **UE source path** | `Runtime/PakFile/Private/PakFile.cpp` (archaic, rarely used) |
| **How it works**  | Pak file includes a CRC table. Verifier computes CRC over pak data and compares. |
| **Detection**     | Look for the standard CRC‑32 polynomial `$hex:04C11DB7` (32-bit little-endian) or CRC‑64 polynomial. Also look for `"CRC32"` string. |
| **Patternsleuth** | `patternsleuth -f SCUMServer.exe -s "CRC32"`<br/>`patternsleuth -f SCUMServer.exe -s "$hex:b7,1d,c1,04"` (CRC‑32 poly LE) |
| **Remarks**       | Extremely weak – provides no integrity against tampering. Unlikely to be used in a production title. |

## 5. No Signature – Only Structural Validation

| Property          | Detail                                   |
|-------------------|------------------------------------------|
| **UE source path** | `Runtime/PakFile/Private/PakFile.cpp` (default path for unsigned paks) |
| **How it works**  | Pak is loaded without any signature check. The only verification is header integrity (magic number, version, index offset). |
| **Detection**     | Absence of any signature‑related constants. If all above scans return zero hits, this is the remaining possibility. |
| **Patternsleuth** | Inverse of all above – no matches. |
| **Remarks**       | SCUM clearly *does* perform some additional check (failure at layer 3). This mode is ruled out. |

## 6. Custom RSA (Non‑standard Encoding)

| Property          | Detail                                   |
|-------------------|------------------------------------------|
| **UE source path** | Not in UE source; engine ships only PKCS#1. |
| **How it works**  | RSA public key stored as raw n and e without ASN.1 framing. Possibly saved as a blob of 256‑byte modulus + 4‑byte exponent. |
| **Detection**     | Search for a 256‑byte block with exponent 65537 (`$hex:01,00,01`). The exponent often appears right after the modulus. |
| **Patternsleuth** | `patternsleuth -f SCUMServer.exe -s "$hex:01,00,01"` (65537 in big‑endian). Then check if it is preceded by 256 bytes of random‑looking data. |
| **Remarks**       | The initial scan found two hits for `02 03 01 00 01` (DER tag+length+exp) but those were data‑table coincidences. A raw `010001` (or `00 01 00 01` in LE) might indicate non‑DER RSA. |

---

## Decision Tree

```
Start with cheap scans (strings, small constants)
  ├─ "SHA1" found?          → FSHA1 chunk signing (Mode 1)
  ├─ ipad/opad found?       → HMAC-SHA256 (Mode 2)
  ├─ "CRC32" or poly?       → CRC embedded (Mode 4)
  └─ None of the above?
        └─ Scan for 32-byte high-entropy blocks in .rdata
              ├─ Single block?                → AES key comparison (Mode 3)
              ├─ 256-byte block with 01 00 01? → Raw RSA (Mode 6)
              └─ No blocks?                   → Impossible; must be something else
```

If multiple hits occur, compare code cross‑references to `FPakPlatformFile::CheckPakSignature` or to the string `"Unable to create pak"`. The function that calls that log is the verifier – check its callers for concrete algorithm usage.

---

## Recommended Scan Order

| Priority | Mode            | Scan Cost | Probability | Reason                                                                 |
|----------|-----------------|-----------|-------------|------------------------------------------------------------------------|
| 1        | **HMAC‑SHA256**  | Low       | High        | Symmetric key signing is popular in custom anti‑tamper. Look for ipad/opad constants. |
| 2        | **Raw RSA**      | Medium    | Medium      | SCUM might store RSA in a non‑standard format. Scan for `01 00 01` + 256‑byte blob. |
| 3        | **AES key**      | Medium    | Low‑Medium  | AES key used for signature would be distinct from data encryption key. Entropy scan. |
| 4        | **SHA1**         | Very Low  | Low         | Unlikely for a 2022 game.                                               |
| 5        | **CRC**          | Low       | Very Low    | Does not provide signing.                                               |

**First pass** – Run these three `patternsleuth` commands:

```bash
# HMAC ipad (64 bytes of 0x36)
patternsleuth -f SCUMServer.exe -s "$hex:36,64"
# HMAC opad (64 bytes of 0x5c)  
patternsleuth -f SCUMServer.exe -s "$hex:5c,64"
# Raw exponent 65537 big-endian
patternsleuth -f SCUMServer.exe -s "$hex:01,00,01"
```

If any of these produce a single hit within `.rdata`, investigate further with a disassembler. The absence of all suggests the signature verification is performed via a **hardware‑protected** key or a **remote attestation** call (e.g., sending hash to a server). In that case, the binary itself contains no key – only a URL or a callback.

---

## Conclusion

Based on the lack of standard RSA, SCUM’s pak signature scheme is most likely **HMAC‑SHA256** with an embedded key, or a **raw RSA** blob. The HMAC theory is the strongest because it is simple to implement, provides symmetric verification, and matches the “integrity‑compromised” bypass pattern (you can replace the key in the binary). Begin with the HMAC ipad/opad scan.