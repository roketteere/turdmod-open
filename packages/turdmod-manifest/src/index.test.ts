import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  ManifestSchema,
  SidecarSchema,
  isCompatibleWithBuild,
  isModeAllowedInEnvironment,
  validateManifest,
  validateSidecar,
} from "./index.js";

describe("ManifestSchema", () => {
  it("accepts a minimal valid manifest", () => {
    const r = validateManifest({
      id: "my-mod",
      name: "My Mod",
      version: "1.0.0",
      mode: "pak-content",
    });
    assert.equal(r.ok, true);
    if (r.ok) {
      assert.equal(r.value.schema, 1);
      assert.deepEqual(r.value.dependencies, {});
      assert.deepEqual(r.value.capabilities, []);
      assert.deepEqual(r.value.tags, []);
    }
  });

  it("rejects an invalid id", () => {
    const r = validateManifest({ id: "Bad_Id", name: "x", version: "1.0.0", mode: "pak-content" });
    assert.equal(r.ok, false);
  });

  it("rejects an unknown mode", () => {
    const r = validateManifest({ id: "x", name: "y", version: "1.0.0", mode: "weird" });
    assert.equal(r.ok, false);
  });

  it("rejects a malformed version", () => {
    const r = validateManifest({ id: "x", name: "y", version: "v1", mode: "pak-content" });
    assert.equal(r.ok, false);
  });

  it("rejects unknown fields (strict)", () => {
    const r = validateManifest({ id: "x-y", name: "y", version: "1.0.0", mode: "pak-content", extra: 1 });
    assert.equal(r.ok, false);
  });
});

describe("SidecarSchema", () => {
  it("requires installedAt", () => {
    const r = validateSidecar({ id: "x-y", name: "y", version: "1.0.0", mode: "pak-content" });
    assert.equal(r.ok, false);
  });

  it("accepts a complete sidecar", () => {
    const r = validateSidecar({
      id: "x-y",
      name: "y",
      version: "1.0.0",
      mode: "pak-content",
      installedAt: "2026-05-09T00:00:00.000Z",
      sourcePath: "C:/path/y.pak",
    });
    assert.equal(r.ok, true);
  });
});

describe("isCompatibleWithBuild", () => {
  it("accepts builds within range", () => {
    assert.equal(isCompatibleWithBuild({ minBuild: "23000000", maxBuild: "" }, "23128448"), true);
    assert.equal(isCompatibleWithBuild({ minBuild: "23128448", maxBuild: "23128448" }, "23128448"), true);
  });
  it("rejects builds below min", () => {
    assert.equal(isCompatibleWithBuild({ minBuild: "23200000", maxBuild: "" }, "23128448"), false);
  });
  it("rejects builds above max", () => {
    assert.equal(isCompatibleWithBuild({ minBuild: "", maxBuild: "23000000" }, "23128448"), false);
  });
  it("treats empty bounds as unbounded", () => {
    assert.equal(isCompatibleWithBuild({ minBuild: "", maxBuild: "" }, "23128448"), true);
  });
});

describe("isModeAllowedInEnvironment", () => {
  it("allows pak-content on private-be-on", () => {
    assert.equal(isModeAllowedInEnvironment("pak-content", "private-be-on"), true);
  });
  it("blocks offline-only on private-be-on", () => {
    assert.equal(isModeAllowedInEnvironment("offline-only", "private-be-on"), false);
  });
  it("blocks pak-content on official servers", () => {
    assert.equal(isModeAllowedInEnvironment("pak-content", "official"), false);
  });
  it("allows external-tool everywhere", () => {
    for (const env of ["solo", "private-be-off", "private-be-on", "official"] as const) {
      assert.equal(isModeAllowedInEnvironment("external-tool", env), true);
    }
  });
});

// Quiet "unused import" if we don't add more direct schema assertions later.
void ManifestSchema; void SidecarSchema;
