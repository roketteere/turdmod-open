// INI parser, serializer, and patch-applier for SCUM ServerSettings.ini.
//
// Section-aware. Preserves the original line ordering, blank lines, and
// comments — only patched key lines are rewritten in place. Unknown keys
// in the patch are appended to the end of their declared section.

export interface IniSection {
  name: string;
  keys: Array<{ key: string; value: string }>;
}

export interface ParsedIni {
  sections: IniSection[];
  rawLines: string[];
  keyIndex: Map<string, { sectionIndex: number; lineIndex: number }>;
}

export type InferredType = 'bool' | 'int' | 'float' | 'duration' | 'text';

export function inferType(value: string): InferredType {
  if (value === 'True' || value === 'False') return 'bool';
  if (/^-?\d+$/.test(value)) return 'int';
  if (/^-?\d+\.\d+$/.test(value)) return 'float';
  if (/^\d{1,4}:\d{2}(:\d{2})?$/.test(value)) return 'duration';
  return 'text';
}

export function parseIni(text: string): ParsedIni {
  const rawLines = text.split(/\r?\n/);
  const sections: IniSection[] = [];
  const keyIndex = new Map<string, { sectionIndex: number; lineIndex: number }>();

  let currentSection: IniSection | null = null;
  let currentSectionIndex = -1;

  for (let lineIndex = 0; lineIndex < rawLines.length; lineIndex++) {
    const line = rawLines[lineIndex];
    const trimmed = line.trimStart();

    const sectionMatch = trimmed.match(/^\[([^\]]+)\]/);
    if (sectionMatch) {
      currentSection = { name: sectionMatch[1], keys: [] };
      sections.push(currentSection);
      currentSectionIndex = sections.length - 1;
      continue;
    }

    if (!trimmed || trimmed.startsWith(';') || trimmed.startsWith('#')) {
      continue;
    }

    const eqIdx = trimmed.indexOf('=');
    if (eqIdx === -1 || currentSection === null) continue;

    const key = trimmed.slice(0, eqIdx).trimEnd();
    // Don't trim the value — embedded leading/trailing whitespace is rare
    // but legal, and SCUM's serializer preserves quotes around URLs etc.
    const value = trimmed.slice(eqIdx + 1);

    currentSection.keys.push({ key, value });

    const compoundKey = `[${currentSection.name}].${key}`;
    if (!keyIndex.has(compoundKey)) {
      keyIndex.set(compoundKey, { sectionIndex: currentSectionIndex, lineIndex });
    }
  }

  return { sections, rawLines, keyIndex };
}

export function applyPatch(raw: string, patch: Map<string, string>): string {
  if (patch.size === 0) return raw;

  const lines = raw.split(/\r?\n/);
  const { keyIndex } = parseIni(raw);

  const applied = new Set<string>();

  for (const [compoundKey, newValue] of patch) {
    const entry = keyIndex.get(compoundKey);
    if (entry !== undefined) {
      const { lineIndex } = entry;
      const original = lines[lineIndex];
      const eqIdx = original.indexOf('=');
      if (eqIdx !== -1) {
        const keyPart = original.slice(0, eqIdx + 1);
        lines[lineIndex] = `${keyPart}${newValue}`;
        applied.add(compoundKey);
      }
    }
  }

  // Append unknown patch keys to the end of their declared section.
  for (const [compoundKey, newValue] of patch) {
    if (applied.has(compoundKey)) continue;

    const bracketEnd = compoundKey.indexOf('].');
    if (bracketEnd === -1) continue;
    const sectionName = compoundKey.slice(1, bracketEnd);
    const key = compoundKey.slice(bracketEnd + 2);

    let sectionStart = -1;
    let insertAt = lines.length;

    for (let i = 0; i < lines.length; i++) {
      const m = lines[i].trimStart().match(/^\[([^\]]+)\]/);
      if (m) {
        if (m[1] === sectionName) {
          sectionStart = i;
        } else if (sectionStart !== -1) {
          insertAt = i;
          break;
        }
      }
    }

    lines.splice(insertAt, 0, `${key}=${newValue}`);
  }

  return lines.join('\n');
}
