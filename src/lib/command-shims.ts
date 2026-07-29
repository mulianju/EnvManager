import type { CommandShim, CommandShimInput, CommandShimStatus } from "../types";

export interface CommandPreviewPart {
  kind: "executable" | "fixedArgument" | "runtimeArguments";
  value: string;
}

export function filterCommandShims(items: CommandShim[], query: string): CommandShim[] {
  const normalized = query.trim().toLocaleLowerCase();
  if (!normalized) return items;
  return items.filter((item) =>
    [item.commandName, item.executable, ...item.fixedArguments]
      .some((value) => value.toLocaleLowerCase().includes(normalized)),
  );
}

export function commandPreviewParts(input: CommandShimInput): CommandPreviewPart[] {
  return [
    { kind: "executable", value: input.executable || "Select an executable" },
    ...input.fixedArguments.map((value) => ({
      kind: "fixedArgument" as const,
      value: value || "(empty argument)",
    })),
    { kind: "runtimeArguments", value: "<runtime arguments>" },
  ];
}

export function commandShimStatusLabel(status: CommandShimStatus): string {
  return {
    ready: "Ready",
    missingExecutable: "Missing executable",
    missingTarget: "Missing target",
    nameConflict: "Name conflict",
    externallyModified: "Externally modified",
    missingShim: "Missing shim",
  }[status];
}

export function emptyCommandShimInput(): CommandShimInput {
  return {
    id: null,
    commandName: "",
    executable: "",
    fixedArguments: [],
  };
}
