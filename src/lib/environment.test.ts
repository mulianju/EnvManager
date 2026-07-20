import { describe, expect, it } from "vitest";
import {
  duplicatePathEntryIndexes,
  filterVariables,
  isSensitiveVariable,
  joinPathEntries,
  parsePathEntries,
} from "./environment";

describe("environment helpers", () => {
  it("round trips PATH entries", () => {
    const entries = parsePathEntries(" C:\\Tools ;%JAVA_HOME%\\bin;;");
    expect(entries).toEqual(["C:\\Tools", "%JAVA_HOME%\\bin"]);
    expect(joinPathEntries(entries)).toBe("C:\\Tools;%JAVA_HOME%\\bin");
  });

  it("detects case-insensitive PATH duplicates", () => {
    expect(duplicatePathEntryIndexes(["C:\\Tools", "c:\\tools\\"])).toEqual([0, 1]);
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
