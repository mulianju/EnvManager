import type { EnvironmentVariable } from "../types";

export function parsePathEntries(value: string): string[] {
  return value
    .split(";")
    .map((entry) => entry.trim())
    .filter(Boolean);
}

export function joinPathEntries(entries: string[]): string {
  return entries.map((entry) => entry.trim()).filter(Boolean).join(";");
}

export function normalizePathEntry(entry: string): string {
  let normalized = entry
    .trim()
    .replace(/^"|"$/g, "")
    .replace(/\//g, "\\")
    .toLowerCase();
  while (normalized.length > 3 && normalized.endsWith("\\")) {
    normalized = normalized.slice(0, -1);
  }
  return normalized;
}

export function duplicatePathEntryIndexes(entries: string[]): number[] {
  const groups = new Map<string, number[]>();
  entries.forEach((entry, index) => {
    const normalized = normalizePathEntry(entry);
    groups.set(normalized, [...(groups.get(normalized) ?? []), index]);
  });
  return [...groups.values()]
    .filter((indexes) => indexes.length > 1)
    .flat()
    .sort((left, right) => left - right);
}

export function isPathVariable(name: string): boolean {
  return name.toLowerCase() === "path";
}

export function isSensitiveVariable(name: string): boolean {
  return /(token|secret|password|passwd|api[_-]?key|private[_-]?key)/i.test(name);
}

export function filterVariables(
  variables: EnvironmentVariable[],
  query: string,
): EnvironmentVariable[] {
  const normalized = query.trim().toLowerCase();
  if (!normalized) return variables;
  return variables.filter(
    (variable) =>
      variable.name.toLowerCase().includes(normalized) ||
      variable.value.toLowerCase().includes(normalized),
  );
}
