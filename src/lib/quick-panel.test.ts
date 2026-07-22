import { describe, expect, it } from "vitest";
import type {
  EffectiveEnvironmentVariable,
  FavoriteKey,
} from "../types";
import {
  buildQuickRows,
  commitQuickDisclosure,
  nextQuickSelectedId,
  nextQuickSelection,
  quickCopyValue,
  quickDisplayValue,
  quickFavoriteKey,
  quickSelectionAnnouncement,
  resetQuickDisclosure,
  shouldHandleQuickKey,
  shouldRefreshQuick,
} from "./quick-panel";

const effectiveVariables: EffectiveEnvironmentVariable[] = [
  effective("Z_USER_TOKEN", "raw-secret", "user"),
  effective("SystemRoot", "C:\\Windows", "system"),
  effective("Path", "C:\\Windows;C:\\UserBin", "combined"),
  effective("JAVA_HOME", "C:\\Java", "user"),
];

describe("quick panel row model", () => {
  it("filters Effective variables by name or value", () => {
    expect(buildQuickRows(effectiveVariables, [], "java").map(({ name }) => name))
      .toEqual(["JAVA_HOME"]);
    expect(buildQuickRows(effectiveVariables, [], "userbin").map(({ name }) => name))
      .toEqual(["Path"]);
    expect(buildQuickRows(effectiveVariables, [], "  ").map(({ name }) => name))
      .toEqual(effectiveVariables.map(({ name }) => name));
  });

  it("puts favorites first while preserving the Effective order within each group", () => {
    const favorites: FavoriteKey[] = [
      { scope: "user", name: "JAVA_HOME" },
      { scope: "user", name: "Z_USER_TOKEN" },
    ];

    const rows = buildQuickRows(effectiveVariables, favorites, "");

    expect(rows.map(({ name }) => name)).toEqual([
      "Z_USER_TOKEN",
      "JAVA_HOME",
      "SystemRoot",
      "Path",
    ]);
    expect(rows.map(({ isFavorite }) => isFavorite)).toEqual([
      true,
      true,
      false,
      false,
    ]);
  });

  it("maps single-source rows to their persisted favorite identity", () => {
    expect(quickFavoriteKey(effectiveVariables[0])).toEqual({
      scope: "user",
      name: "Z_USER_TOKEN",
    });
    expect(quickFavoriteKey(effectiveVariables[1])).toEqual({
      scope: "system",
      name: "SystemRoot",
    });
    expect(quickFavoriteKey(effectiveVariables[2])).toBeNull();
  });

  it("treats combined PATH as favorite when either underlying scope is pinned", () => {
    const userFavorite = buildQuickRows(effectiveVariables, [
      { scope: "user", name: "PATH" },
    ], "");
    const systemFavorite = buildQuickRows(effectiveVariables, [
      { scope: "system", name: "path" },
    ], "");

    expect(userFavorite[0]).toMatchObject({ name: "Path", isFavorite: true });
    expect(systemFavorite[0]).toMatchObject({ name: "Path", isFavorite: true });
  });

  it("requires the reconciled display name for single-source favorites", () => {
    const rows = buildQuickRows(effectiveVariables, [
      { scope: "user", name: "z_user_token" },
    ], "");

    expect(rows.find(({ name }) => name === "Z_USER_TOKEN")?.isFavorite).toBe(false);
  });

  it("masks sensitive display values without changing reveal or copied values", () => {
    const [row] = buildQuickRows(effectiveVariables, [], "token");

    expect(row).toMatchObject({
      name: "Z_USER_TOKEN",
      value: "raw-secret",
      isSensitive: true,
    });
    expect(quickDisplayValue(row, false)).not.toContain("raw-secret");
    expect(quickDisplayValue(row, true)).toBe("raw-secret");
    expect(quickCopyValue(row)).toBe("raw-secret");
  });
});

describe("quick panel keyboard selection", () => {
  it("wraps ArrowUp and ArrowDown and starts from the nearest edge", () => {
    expect(nextQuickSelection(-1, "ArrowDown", 3)).toBe(0);
    expect(nextQuickSelection(2, "ArrowDown", 3)).toBe(0);
    expect(nextQuickSelection(-1, "ArrowUp", 3)).toBe(2);
    expect(nextQuickSelection(0, "ArrowUp", 3)).toBe(2);
  });

  it("supports Home and End and never returns an out-of-bounds selection", () => {
    expect(nextQuickSelection(2, "Home", 4)).toBe(0);
    expect(nextQuickSelection(0, "End", 4)).toBe(3);
    expect(nextQuickSelection(99, "Enter", 3)).toBe(2);
    expect(nextQuickSelection(-1, "Escape", 3)).toBe(-1);
    expect(nextQuickSelection(0, "ArrowDown", 0)).toBe(-1);
  });

  it("keeps selection by row identity across favorite reordering", () => {
    const initialRows = buildQuickRows(effectiveVariables, [], "");
    const selectedId = initialRows.find(({ name }) => name === "SystemRoot")!.id;
    const reorderedRows = buildQuickRows(effectiveVariables, [
      { scope: "system", name: "SystemRoot" },
    ], "");

    expect(nextQuickSelectedId(reorderedRows, selectedId, "Enter")).toBe(selectedId);
  });

  it("clears missing identities and wraps navigation using the visible rows", () => {
    const filteredRows = buildQuickRows(effectiveVariables, [], "java");
    expect(nextQuickSelectedId(filteredRows, "system:SystemRoot", "Enter")).toBeNull();

    const allRows = buildQuickRows(effectiveVariables, [], "");
    const lastRow = allRows[allRows.length - 1];
    expect(nextQuickSelectedId(allRows, lastRow.id, "ArrowDown"))
      .toBe(allRows[0].id);
    expect(nextQuickSelectedId(allRows, allRows[0].id, "ArrowUp"))
      .toBe(lastRow.id);
  });

  it("preserves Home and End cursor behavior for a non-empty search input", () => {
    expect(shouldHandleQuickKey("Home", true, true)).toBe(false);
    expect(shouldHandleQuickKey("End", true, true)).toBe(false);
    expect(shouldHandleQuickKey("Home", true, false)).toBe(true);
    expect(shouldHandleQuickKey("ArrowDown", true, true)).toBe(true);
  });
});

describe("quick panel sensitive disclosure", () => {
  it("clears revealed row identities without retaining them for refreshed values", () => {
    const previous = new Set(["user:Z_USER_TOKEN"]);
    const reset = resetQuickDisclosure(previous);

    expect(reset).toEqual(new Set());
    expect(previous).toEqual(new Set(["user:Z_USER_TOKEN"]));
  });

  it("commits every refreshed snapshot with empty disclosure state", () => {
    expect(commitQuickDisclosure(new Set(["user:Z_USER_TOKEN"])))
      .toEqual(new Set());
  });
});

describe("quick panel selection announcement", () => {
  it("announces only identity and source for sensitive and regular rows", () => {
    const rows = buildQuickRows(effectiveVariables, [], "");
    const sensitive = rows.find(({ name }) => name === "Z_USER_TOKEN")!;
    const regular = rows.find(({ name }) => name === "JAVA_HOME")!;

    expect(quickSelectionAnnouncement(sensitive)).toBe(
      "Selected Z_USER_TOKEN, user. Press Enter to copy.",
    );
    expect(quickSelectionAnnouncement(sensitive)).not.toContain("raw-secret");
    expect(quickSelectionAnnouncement(regular)).toBe(
      "Selected JAVA_HOME, user. Press Enter to copy.",
    );
    expect(quickSelectionAnnouncement(null)).toBe("");
  });
});

describe("quick panel refresh decision", () => {
  it("polls only changed revisions but always refreshes when the window regains focus", () => {
    expect(shouldRefreshQuick("rev-1", "rev-1", "poll")).toBe(false);
    expect(shouldRefreshQuick("rev-1", "rev-2", "poll")).toBe(true);
    expect(shouldRefreshQuick("rev-1", "rev-1", "focus")).toBe(true);
  });
});

function effective(
  name: string,
  value: string,
  source: "user" | "system" | "combined",
): EffectiveEnvironmentVariable {
  return {
    name,
    value,
    valueType: "string",
    source,
    shadowed: false,
    conflict: false,
  };
}
