// Wizard state machine. One shared store — the AI assistant reads it to know
// what's already been done, and writes to it when it performs a step, so the
// UI reflects the assistant's work exactly as if you'd clicked the buttons.

import { createContext, useContext } from "react";
import type { CapabilityReport, DetectedInstalls, HostKind, StepResult, VerifyReport } from "./api";

export const STEPS = [
  { id: "welcome", label: "Start" },
  { id: "detect", label: "Find server" },
  { id: "capability", label: "What you can run" },
  { id: "configure", label: "Settings" },
  { id: "install", label: "Install" },
  { id: "verify", label: "Check" },
] as const;

export type StepId = (typeof STEPS)[number]["id"];

export interface SetupState {
  step: StepId;
  hostKind: HostKind | null;
  detected: DetectedInstalls | null;
  /** The SCUM server folder we're installing into. */
  serverRoot: string;
  capability: CapabilityReport | null;
  port: number;
  token: string;
  config: Record<string, unknown> | null;
  artifactsDir: string | null;
  installResults: StepResult[];
  verifyReport: VerifyReport | null;
  /** Last error text — fed to the assistant so it can diagnose without asking. */
  lastError: string;
}

export const initialState: SetupState = {
  step: "welcome",
  hostKind: null,
  detected: null,
  serverRoot: "",
  capability: null,
  port: 9090,
  token: "",
  config: null,
  artifactsDir: null,
  installResults: [],
  verifyReport: null,
  lastError: "",
};

export interface SetupStore {
  state: SetupState;
  set: (patch: Partial<SetupState>) => void;
  go: (step: StepId) => void;
  next: () => void;
  back: () => void;
}

export const SetupContext = createContext<SetupStore | null>(null);

export function useSetup(): SetupStore {
  const s = useContext(SetupContext);
  if (!s) throw new Error("useSetup outside SetupContext");
  return s;
}

export function stepIndex(id: StepId): number {
  return STEPS.findIndex((s) => s.id === id);
}

/** Steps the user can jump back to — anything at or before where they've been. */
export function canVisit(target: StepId, current: StepId): boolean {
  return stepIndex(target) <= stepIndex(current);
}

/** Plain-language summary of state for the assistant's system prompt. */
export function describeState(s: SetupState): string {
  const lines = [
    `Current wizard step: ${s.step}`,
    `Host type: ${s.hostKind ?? "not chosen yet"}`,
    `SCUM server folder: ${s.serverRoot || "not set"}`,
    `Server Pack artifacts folder: ${s.artifactsDir ?? "not found yet"}`,
    `Service port: ${s.port}`,
    `Config prepared: ${s.config ? "yes" : "no"}`,
  ];
  if (s.capability) {
    lines.push(
      `Capability verdict: ${s.capability.verdict}`,
      ...s.capability.capabilities.map((c) => `  - ${c.label}: ${c.support}${c.reason ? ` (${c.reason})` : ""}`),
    );
  }
  if (s.installResults.length) {
    lines.push(
      "Install results:",
      ...s.installResults.map((r) => `  - ${r.step}: ${r.ok ? "ok" : "FAILED"} — ${r.detail}`),
    );
  }
  if (s.verifyReport) {
    lines.push(
      `Verify: ${s.verifyReport.summary}`,
      ...s.verifyReport.checks.map((c) => `  - ${c.label}: ${c.ok ? "ok" : "FAILED"} — ${c.detail}`),
    );
  }
  if (s.lastError) lines.push(`Last error: ${s.lastError}`);
  return lines.join("\n");
}
