import { describe, expect, it } from "vitest";
import type { CommandShim, CommandShimInput } from "../types";
import {
  commandShimAccessNeedsRepair,
  commandPreviewParts,
  commandShimStatusLabel,
  filterCommandShims,
} from "./command-shims";

const items: CommandShim[] = [
  {
    id: "sharedev-id",
    commandName: "sharedev",
    executable: "C:\\Program Files\\Node\\node.exe",
    fixedArguments: ["D:\\tools\\sharedev.js", "--channel=dev"],
    shimPath: "C:\\Managed\\sharedev.cmd",
    status: "ready",
    statusMessage: null,
    createdAtMs: 1,
    updatedAtMs: 2,
  },
  {
    id: "python-id",
    commandName: "local-python",
    executable: "D:\\Python\\python.exe",
    fixedArguments: [],
    shimPath: "C:\\Managed\\local-python.cmd",
    status: "missingExecutable",
    statusMessage: "Executable is missing",
    createdAtMs: 3,
    updatedAtMs: 4,
  },
];

describe("Command Shim workflow", () => {
  it("filters by command, executable, and every fixed argument", () => {
    expect(filterCommandShims(items, "SHARE")).toEqual([items[0]]);
    expect(filterCommandShims(items, "python.exe")).toEqual([items[1]]);
    expect(filterCommandShims(items, "channel=dev")).toEqual([items[0]]);
    expect(filterCommandShims(items, "  ")).toBe(items);
  });

  it("builds a structured preview without flattening arguments into shell text", () => {
    const input: CommandShimInput = {
      id: null,
      commandName: "sharedev",
      executable: "C:\\Program Files\\Node\\node.exe",
      fixedArguments: ["D:\\tool path\\sharedev.js", ""],
    };

    expect(commandPreviewParts(input)).toEqual([
      { kind: "executable", value: "C:\\Program Files\\Node\\node.exe" },
      { kind: "fixedArgument", value: "D:\\tool path\\sharedev.js" },
      { kind: "fixedArgument", value: "(empty argument)" },
      { kind: "runtimeArguments", value: "<runtime arguments>" },
    ]);
  });

  it("maps every backend status to a concise label", () => {
    expect(commandShimStatusLabel("ready")).toBe("Ready");
    expect(commandShimStatusLabel("externallyModified")).toBe("Externally modified");
    expect(commandShimStatusLabel("missingShim")).toBe("Missing shim");
  });

  it("offers shell repair only when PATH or a managed wrapper is missing", () => {
    const snapshot = {
      items,
      managedDirectory: "C:\\Managed",
      pathReady: true,
    };
    expect(commandShimAccessNeedsRepair(snapshot)).toBe(false);
    expect(commandShimAccessNeedsRepair({ ...snapshot, items: [], pathReady: false })).toBe(false);
    expect(commandShimAccessNeedsRepair({ ...snapshot, pathReady: false })).toBe(true);
    expect(commandShimAccessNeedsRepair({
      ...snapshot,
      items: [{ ...items[0], status: "missingShim" }],
    })).toBe(true);
    expect(commandShimAccessNeedsRepair({
      ...snapshot,
      items: [{ ...items[0], status: "externallyModified" }],
    })).toBe(false);
  });
});
