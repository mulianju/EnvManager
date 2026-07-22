import { describe, expect, it } from "vitest";
import {
  deduplicatePathEntries,
  duplicatePathEntryIndexes,
  filterVariables,
  filterPathEntryStatuses,
  formatVariableForCopy,
  isSensitiveVariable,
  joinPathEntries,
  mergeEffectiveVariables,
  parsePathBulkInput,
  parsePathEntries,
  summarizePathChanges,
} from "./environment";
import type { EnvironmentVariable, PathEntryStatus } from "../types";

function variable(
  name: string,
  value: string,
  scope: "user" | "system",
  valueType: "string" | "expandableString" = "string",
): EnvironmentVariable {
  return { name, value, valueType, scope };
}

describe("environment helpers", () => {
  it("round trips PATH entries", () => {
    const entries = parsePathEntries(" C:\\Tools ;%JAVA_HOME%\\bin;;");
    expect(entries).toEqual(["C:\\Tools", "%JAVA_HOME%\\bin"]);
    expect(joinPathEntries(entries)).toBe("C:\\Tools;%JAVA_HOME%\\bin");
  });

  it("detects case-insensitive PATH duplicates", () => {
    expect(duplicatePathEntryIndexes(["C:\\Tools", "c:\\tools\\"])).toEqual([0, 1]);
  });

  it("parses multiline and semicolon-delimited PATH input", () => {
    expect(
      parsePathBulkInput(
        " C:\\Tools ; D:/Node/bin\r\n\r\n%JAVA_HOME%\\bin\n E:\\Apps ; ",
      ),
    ).toEqual(["C:\\Tools", "D:/Node/bin", "%JAVA_HOME%\\bin", "E:\\Apps"]);
    expect(parsePathBulkInput(" \r\n ; \n; ")).toEqual([]);
  });

  it("deduplicates PATH entries using Windows identity and keeps the first entry", () => {
    expect(
      deduplicatePathEntries([
        "C:\\Tools\\",
        "c:/tools",
        "D:\\Bin",
        "d:/BIN/",
        "%JAVA_HOME%\\bin",
        "%java_home%/BIN/",
      ]),
    ).toEqual(["C:\\Tools\\", "D:\\Bin", "%JAVA_HOME%\\bin"]);
    expect(deduplicatePathEntries(["C:\\Tools", "c:/tools/", "C:\\TOOLS\\"])).toEqual([
      "C:\\Tools",
    ]);
    expect(deduplicatePathEntries([])).toEqual([]);
  });

  it("filters PATH statuses without dropping duplicate missing entries", () => {
    const statuses: PathEntryStatus[] = [
      { value: "C:\\OK", expandedValue: "C:\\OK", exists: true, duplicate: false },
      { value: "C:\\Duplicate", expandedValue: "C:\\Duplicate", exists: true, duplicate: true },
      { value: "C:\\Missing", expandedValue: "C:\\Missing", exists: false, duplicate: false },
      { value: "c:\\missing", expandedValue: "c:\\missing", exists: false, duplicate: true },
    ];

    expect(filterPathEntryStatuses(statuses, "all")).toEqual(statuses);
    expect(filterPathEntryStatuses(statuses, "duplicate")).toEqual([
      statuses[1],
      statuses[3],
    ]);
    expect(filterPathEntryStatuses(statuses, "missing")).toEqual([
      statuses[2],
      statuses[3],
    ]);
  });

  it("summarizes additions, removals, and order-only PATH changes", () => {
    expect(
      summarizePathChanges(
        ["C:\\Tools", "D:\\Old", "%JAVA_HOME%\\bin"],
        ["c:/tools/", "%JAVA_HOME%/BIN/", "E:\\New"],
      ),
    ).toEqual({
      added: ["E:\\New"],
      removed: ["D:\\Old"],
      orderChanged: false,
    });
    expect(
      summarizePathChanges(["C:\\One", "D:\\Two"], ["D:/Two/", "c:/one/"]),
    ).toEqual({ added: [], removed: [], orderChanged: true });
    expect(summarizePathChanges(["C:\\Tools\\"], ["c:/TOOLS"])).toEqual({
      added: [],
      removed: [],
      orderChanged: false,
    });
    expect(summarizePathChanges([], [])).toEqual({
      added: [],
      removed: [],
      orderChanged: false,
    });
    expect(summarizePathChanges(["C:\\Tools"], ["C:\\Tools", "c:/tools/"])).toEqual({
      added: ["c:/tools/"],
      removed: [],
      orderChanged: false,
    });
    expect(summarizePathChanges(["C:\\Tools", "c:/tools/"], ["C:\\Tools"])).toEqual({
      added: [],
      removed: ["c:/tools/"],
      orderChanged: false,
    });
  });

  it("merges effective variables with User precedence and a combined PATH", () => {
    const effective = mergeEffectiveVariables(
      [
        variable("JAVA_HOME", "C:\\UserJava", "user"),
        variable("Path", "C:\\UserBin", "user"),
        variable("USER_ONLY", "user", "user"),
      ],
      [
        variable("java_home", "C:\\SystemJava", "system"),
        variable("PATH", "C:\\SystemBin", "system", "expandableString"),
        variable("SYSTEM_ONLY", "system", "system"),
      ],
    );

    expect(effective.find(({ name }) => name.toLowerCase() === "java_home")).toMatchObject({
      name: "JAVA_HOME",
      value: "C:\\UserJava",
      scope: "user",
      source: "user",
      shadowed: true,
      conflict: true,
    });
    expect(effective.find(({ name }) => name.toLowerCase() === "path")).toMatchObject({
      value: "C:\\SystemBin;C:\\UserBin",
      valueType: "expandableString",
      scope: "combined",
      source: "combined",
      shadowed: false,
      conflict: false,
    });
    expect(effective.find(({ name }) => name === "USER_ONLY")).toMatchObject({
      source: "user",
      shadowed: false,
      conflict: false,
    });
    expect(effective.find(({ name }) => name === "SYSTEM_ONLY")).toMatchObject({
      source: "system",
      shadowed: false,
      conflict: false,
    });
  });

  it("merges effective variables when only one scope is present", () => {
    expect(mergeEffectiveVariables([variable("USER_ONLY", "value", "user")], [])).toEqual([
      expect.objectContaining({ name: "USER_ONLY", source: "user", shadowed: false, conflict: false }),
    ]);
    expect(mergeEffectiveVariables([], [variable("SYSTEM_ONLY", "value", "system")])).toEqual([
      expect.objectContaining({ name: "SYSTEM_ONLY", source: "system", shadowed: false, conflict: false }),
    ]);
  });

  it("only marks shadowed variables as conflicts when value or type differs", () => {
    const same = mergeEffectiveVariables(
      [variable("SAME", "value", "user")],
      [variable("same", "value", "system")],
    )[0];
    const differentValue = mergeEffectiveVariables(
      [variable("VALUE", "user", "user")],
      [variable("value", "system", "system")],
    )[0];
    const differentType = mergeEffectiveVariables(
      [variable("TYPE", "value", "user")],
      [variable("type", "value", "system", "expandableString")],
    )[0];

    expect(same).toMatchObject({ shadowed: true, conflict: false });
    expect(differentValue).toMatchObject({ shadowed: true, conflict: true });
    expect(differentType).toMatchObject({ shadowed: true, conflict: true });
  });

  it("keeps known Windows-distinct sharp-s names separate in browser preview", () => {
    const effective = mergeEffectiveVariables(
      [variable("ß", "lower", "user")],
      [variable("ẞ", "upper", "system")],
    );

    expect(effective.map(({ name }) => name)).toEqual(["ẞ", "ß"]);
    expect(effective.every(({ shadowed }) => !shadowed)).toBe(true);
  });

  it("formats variable copy values and escapes unsafe PowerShell names", () => {
    const regular = variable("JAVA_HOME", "C:\\Java", "user");
    expect(formatVariableForCopy(regular, "name")).toBe("JAVA_HOME");
    expect(formatVariableForCopy(regular, "value")).toBe("C:\\Java");
    expect(formatVariableForCopy(regular, "powershell")).toBe("$env:JAVA_HOME");

    const unsafe = variable("SDK'S HOME", "C:\\SDK", "user");
    expect(formatVariableForCopy(unsafe, "powershell")).toBe(
      "[Environment]::GetEnvironmentVariable('SDK''S HOME')",
    );
  });

  it("recognizes likely sensitive variables", () => {
    expect(isSensitiveVariable("GITHUB_TOKEN")).toBe(true);
    expect(isSensitiveVariable("JAVA_HOME")).toBe(false);
  });

  it("filters by name or value", () => {
    const variables = [
      { name: "JAVA_HOME", value: "C:\\Java", valueType: "string" as const, scope: "user" as const },
      { name: "NODE_HOME", value: "D:\\Node", valueType: "string" as const, scope: "user" as const },
    ];
    expect(filterVariables(variables, "node")).toEqual([variables[1]]);
    expect(filterVariables(variables, "java")).toEqual([variables[0]]);
  });
});
