import { describe, expect, it } from "vitest";
import type {
  EffectiveEnvironmentVariable,
  TransferVariableInput,
} from "../types";
import {
  canTransferVariable,
  filterEffectiveVariables,
  revisionRefreshDecision,
  retryTransferAfterCollision,
  shouldApplyGeneration,
  transferConfirmationMessage,
} from "./environment";

const effectiveVariables: EffectiveEnvironmentVariable[] = [
  {
    name: "JAVA_HOME",
    value: "C:\\UserJava",
    valueType: "string",
    source: "user",
    shadowed: true,
    conflict: true,
  },
  {
    name: "Path",
    value: "C:\\Windows;C:\\UserBin",
    valueType: "expandableString",
    source: "combined",
    shadowed: false,
    conflict: false,
  },
  {
    name: "SystemRoot",
    value: "C:\\Windows",
    valueType: "string",
    source: "system",
    shadowed: false,
    conflict: false,
  },
];

describe("variable view workflow", () => {
  it("filters Effective rows by name or value without losing source and conflict metadata", () => {
    expect(filterEffectiveVariables(effectiveVariables, "java")).toEqual([
      effectiveVariables[0],
    ]);
    expect(filterEffectiveVariables(effectiveVariables, "userbin")).toEqual([
      effectiveVariables[1],
    ]);
    expect(filterEffectiveVariables(effectiveVariables, "  ")).toEqual(
      effectiveVariables,
    );
  });

  it("gates cross-scope transfers by every scope the operation mutates", () => {
    expect(canTransferVariable("system", "copy", false)).toBe(true);
    expect(canTransferVariable("system", "move", false)).toBe(false);
    expect(canTransferVariable("user", "copy", false)).toBe(false);
    expect(canTransferVariable("user", "move", false)).toBe(false);
    expect(canTransferVariable("user", "move", true)).toBe(true);
    expect(canTransferVariable("system", "move", true)).toBe(true);
  });

  it("only requests confirmation for a collision or a move and states the impact", () => {
    expect(
      transferConfirmationMessage(transfer("system", "copy", false)),
    ).toBeNull();

    const collision = transferConfirmationMessage(transfer("system", "copy", true));
    expect(collision).toMatch(/ComSpec/i);
    expect(collision).toMatch(/overwrite/i);
    expect(collision).toMatch(/User/i);

    const move = transferConfirmationMessage(transfer("user", "move", false));
    expect(move).toMatch(/JAVA_HOME/i);
    expect(move).toMatch(/remove/i);
    expect(move).toMatch(/User/i);
    expect(move).toMatch(/System/i);
  });

  it("refreshes external revisions only when no editor or confirmation is open", () => {
    expect(revisionRefreshDecision("rev-1", "rev-1", false)).toBe("unchanged");
    expect(revisionRefreshDecision("rev-1", "rev-2", false)).toBe("refresh");
    expect(revisionRefreshDecision("rev-1", "rev-2", true)).toBe("defer");
  });

  it("only applies the latest async generation", () => {
    expect(shouldApplyGeneration(4, 4)).toBe(true);
    expect(shouldApplyGeneration(3, 4)).toBe(false);
  });

  it("retries transfers with overwrite only for a collision error", () => {
    const input = transfer("system", "copy", false);
    const attempt = { input, expectedRevision: "opened-revision" };
    expect(retryTransferAfterCollision(attempt, "variableAlreadyExists")).toEqual({
      input: { ...input, overwrite: true },
      expectedRevision: "opened-revision",
    });
    expect(retryTransferAfterCollision(attempt, "registryOperationFailed")).toBeNull();
  });
});

function transfer(
  sourceScope: "user" | "system",
  mode: "copy" | "move",
  overwrite: boolean,
): TransferVariableInput {
  return {
    sourceScope,
    targetScope: sourceScope === "user" ? "system" : "user",
    name: sourceScope === "user" ? "JAVA_HOME" : "ComSpec",
    mode,
    overwrite,
  };
}
