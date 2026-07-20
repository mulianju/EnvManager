import type {
  EffectiveEnvironmentVariable,
  EnvironmentVariable,
  PathChangeSummary,
  PathEntryStatus,
  PathStatusFilter,
  VariableCopyFormat,
} from "../types";

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

export function parsePathBulkInput(value: string): string[] {
  return value
    .split(/[;\r\n]+/)
    .map((entry) => entry.trim())
    .filter(Boolean);
}

export function deduplicatePathEntries(entries: string[]): string[] {
  const seen = new Set<string>();
  return entries.filter((entry) => {
    const normalized = normalizePathEntry(entry);
    if (seen.has(normalized)) return false;
    seen.add(normalized);
    return true;
  });
}

export function filterPathEntryStatuses(
  statuses: PathEntryStatus[],
  filter: PathStatusFilter,
): PathEntryStatus[] {
  if (filter === "duplicate") {
    return statuses.filter(({ duplicate }) => duplicate);
  }
  if (filter === "missing") {
    return statuses.filter(({ exists }) => !exists);
  }
  return statuses;
}

export function summarizePathChanges(
  previousEntries: string[],
  currentEntries: string[],
): PathChangeSummary {
  const previousIdentities = previousEntries.map(normalizePathEntry);
  const currentIdentities = currentEntries.map(normalizePathEntry);
  const unmatchedEntries = (
    entries: string[],
    identities: string[],
    comparisonIdentities: string[],
  ): string[] => {
    const available = new Map<string, number>();
    comparisonIdentities.forEach((identity) => {
      available.set(identity, (available.get(identity) ?? 0) + 1);
    });

    return entries.filter((_, index) => {
      const identity = identities[index];
      const count = available.get(identity) ?? 0;
      if (count === 0) return true;
      available.set(identity, count - 1);
      return false;
    });
  };
  const added = unmatchedEntries(currentEntries, currentIdentities, previousIdentities);
  const removed = unmatchedEntries(previousEntries, previousIdentities, currentIdentities);
  const orderChanged =
    added.length === 0 &&
    removed.length === 0 &&
    previousIdentities.some((identity, index) => identity !== currentIdentities[index]);

  return { added, removed, orderChanged };
}

export function mergeEffectiveVariables(
  userVariables: EnvironmentVariable[],
  systemVariables: EnvironmentVariable[],
): EffectiveEnvironmentVariable[] {
  const userByName = new Map(userVariables.map((variable) => [variable.name.toLowerCase(), variable]));
  const systemByName = new Map(
    systemVariables.map((variable) => [variable.name.toLowerCase(), variable]),
  );
  const names = [
    ...systemVariables.map(({ name }) => name.toLowerCase()),
    ...userVariables.map(({ name }) => name.toLowerCase()),
  ];

  return [...new Set(names)].map((name) => {
    const userVariable = userByName.get(name);
    const systemVariable = systemByName.get(name);

    if (isPathVariable(name) && userVariable && systemVariable) {
      return {
        ...userVariable,
        value: joinPathEntries([
          ...parsePathEntries(systemVariable.value),
          ...parsePathEntries(userVariable.value),
        ]),
        valueType:
          userVariable.valueType === "expandableString" ||
          systemVariable.valueType === "expandableString"
            ? "expandableString"
            : "string",
        source: "combined",
        shadowed: false,
        conflict: false,
      };
    }

    if (userVariable) {
      return {
        ...userVariable,
        source: "user",
        shadowed: Boolean(systemVariable),
        conflict: Boolean(systemVariable),
      };
    }

    return {
      ...systemVariable!,
      source: "system",
      shadowed: false,
      conflict: false,
    };
  });
}

export function formatVariableForCopy(
  variable: EnvironmentVariable,
  format: VariableCopyFormat,
): string {
  if (format === "name") return variable.name;
  if (format === "value") return variable.value;
  if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(variable.name)) {
    return `$env:${variable.name}`;
  }
  const escapedName = variable.name.replace(/'/g, "''");
  return `[Environment]::GetEnvironmentVariable('${escapedName}')`;
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
