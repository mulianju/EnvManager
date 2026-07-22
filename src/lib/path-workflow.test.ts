import { describe, expect, it } from "vitest";
import type { PathEntryStatus } from "../types";
import {
  canEditPath,
  insertPathEntries,
  pathFilterCounts,
  parsePathDragIndex,
  removeDuplicatePathEntries,
  reorderPathEntries,
} from "./environment";

describe("PATH editor workflow", () => {
  it("inserts multiline and semicolon-delimited entries without adding normalized duplicates", () => {
    expect(
      insertPathEntries(
        ["C:\\Existing", "%JAVA_HOME%\\bin"],
        "D:\\Tools\r\nc:/existing/; %java_home%/BIN/; E:\\Apps",
      ),
    ).toEqual([
      "C:\\Existing",
      "%JAVA_HOME%\\bin",
      "D:\\Tools",
      "E:\\Apps",
    ]);
  });

  it("leaves PATH unchanged for empty or duplicate-only pasted input", () => {
    const entries = ["C:\\Existing", "D:\\Tools"];

    expect(insertPathEntries(entries, " \r\n ; ")).toEqual(entries);
    expect(insertPathEntries(entries, "c:/existing/; D:/TOOLS/")).toEqual(entries);
  });

  it("reorders dropped entries and ignores invalid or self drops", () => {
    const entries = ["A", "B", "C", "D"];

    expect(reorderPathEntries(entries, 0, 2)).toEqual(["B", "C", "A", "D"]);
    expect(reorderPathEntries(entries, 3, 1)).toEqual(["A", "D", "B", "C"]);
    expect(reorderPathEntries(entries, -1, 2)).toBe(entries);
    expect(reorderPathEntries(entries, 1, 4)).toBe(entries);
    expect(reorderPathEntries(entries, 2, 2)).toBe(entries);
  });

  it("accepts only the current internal drag payload", () => {
    expect(parsePathDragIndex("1", 3, 1)).toBe(1);
    expect(parsePathDragIndex("1junk", 3, 1)).toBeNull();
    expect(parsePathDragIndex("-1", 3, -1)).toBeNull();
    expect(parsePathDragIndex("3", 3, 3)).toBeNull();
    expect(parsePathDragIndex("1", 3, 0)).toBeNull();
    expect(parsePathDragIndex("1", 3, null)).toBeNull();
  });

  it("removes only normalized duplicates and keeps the first entry including missing paths", () => {
    expect(
      removeDuplicatePathEntries([
        "C:\\Missing",
        "c:/missing/",
        "D:\\AlsoMissing",
        "C:\\Present",
      ]),
    ).toEqual(["C:\\Missing", "D:\\AlsoMissing", "C:\\Present"]);
  });

  it("reports independent All, Duplicate, and Missing filter counts", () => {
    const statuses: PathEntryStatus[] = [
      status("C:\\OK", true, false),
      status("C:\\Duplicate", true, true),
      status("C:\\Missing", false, false),
      status("c:\\missing", false, true),
    ];

    expect(pathFilterCounts(statuses)).toEqual({
      all: 4,
      duplicate: 2,
      missing: 2,
    });
  });

  it("disables PATH mutations while busy and for unelevated System edits", () => {
    expect(canEditPath("user", false, false)).toBe(true);
    expect(canEditPath("user", false, true)).toBe(false);
    expect(canEditPath("system", false, false)).toBe(false);
    expect(canEditPath("system", true, false)).toBe(true);
    expect(canEditPath("system", true, true)).toBe(false);
  });
});

function status(
  value: string,
  exists: boolean,
  duplicate: boolean,
): PathEntryStatus {
  return { value, expandedValue: value, exists, duplicate };
}
