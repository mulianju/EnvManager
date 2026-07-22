import type {
  EffectiveEnvironmentVariable,
  FavoriteKey,
} from "../types";
import { isSensitiveVariable } from "./environment";

export interface QuickPanelRow extends EffectiveEnvironmentVariable {
  id: string;
  isFavorite: boolean;
  isSensitive: boolean;
  favoriteKey: FavoriteKey | null;
}

export type QuickSelectionKey =
  | "ArrowDown"
  | "ArrowUp"
  | "Home"
  | "End"
  | "Enter"
  | "Escape";

export function buildQuickRows(
  variables: EffectiveEnvironmentVariable[],
  favorites: FavoriteKey[],
  query: string,
): QuickPanelRow[] {
  const normalizedQuery = query.trim().toLowerCase();
  const rows = variables
    .filter((variable) =>
      !normalizedQuery ||
      variable.name.toLowerCase().includes(normalizedQuery) ||
      variable.value.toLowerCase().includes(normalizedQuery)
    )
    .map((variable) => {
      const favoriteKey = quickFavoriteKey(variable);
      const isFavorite = variable.source === "combined"
        ? isCombinedPathFavorite(variable.name, favorites)
        : favoriteKey !== null && favorites.some((favorite) =>
          favorite.scope === favoriteKey.scope &&
          favorite.name === favoriteKey.name
        );
      return {
        ...variable,
        id: `${variable.source}:${variable.name}`,
        isFavorite,
        isSensitive: isSensitiveVariable(variable.name),
        favoriteKey,
      };
    });

  return [
    ...rows.filter(({ isFavorite }) => isFavorite),
    ...rows.filter(({ isFavorite }) => !isFavorite),
  ];
}

export function quickFavoriteKey(
  variable: EffectiveEnvironmentVariable,
): FavoriteKey | null {
  if (variable.source === "combined") return null;
  return { scope: variable.source, name: variable.name };
}

export function quickDisplayValue(row: QuickPanelRow, revealed: boolean): string {
  return row.isSensitive && !revealed ? "********" : row.value;
}

export function quickCopyValue(row: QuickPanelRow): string {
  return row.value;
}

export function nextQuickSelection(
  currentIndex: number,
  key: QuickSelectionKey,
  rowCount: number,
): number {
  if (rowCount <= 0 || key === "Escape") return -1;
  if (key === "ArrowDown") {
    return currentIndex < 0 || currentIndex >= rowCount - 1
      ? 0
      : currentIndex + 1;
  }
  if (key === "ArrowUp") {
    return currentIndex <= 0 || currentIndex >= rowCount
      ? rowCount - 1
      : currentIndex - 1;
  }
  if (key === "Home") return 0;
  if (key === "End") return rowCount - 1;
  return Math.min(Math.max(currentIndex, -1), rowCount - 1);
}

export function nextQuickSelectedId(
  rows: Array<Pick<QuickPanelRow, "id">>,
  selectedId: string | null,
  key: QuickSelectionKey,
): string | null {
  const currentIndex = selectedId === null
    ? -1
    : rows.findIndex(({ id }) => id === selectedId);
  const nextIndex = nextQuickSelection(currentIndex, key, rows.length);
  return rows[nextIndex]?.id ?? null;
}

export function shouldHandleQuickKey(
  key: QuickSelectionKey,
  isSearchInput: boolean,
  hasQuery: boolean,
): boolean {
  return !(
    isSearchInput &&
    hasQuery &&
    (key === "Home" || key === "End")
  );
}

export function resetQuickDisclosure(
  _revealedRows: ReadonlySet<string>,
): Set<string> {
  return new Set();
}

export function shouldRefreshQuick(
  currentRevision: string,
  observedRevision: string,
  trigger: "poll" | "focus",
): boolean {
  return trigger === "focus" || currentRevision !== observedRevision;
}

function isCombinedPathFavorite(
  combinedName: string,
  favorites: FavoriteKey[],
): boolean {
  return isAsciiPathName(combinedName) &&
    favorites.some(({ name }) => isAsciiPathName(name));
}

function isAsciiPathName(name: string): boolean {
  return asciiLower(name) === "path";
}

function asciiLower(value: string): string {
  return value.replace(/[A-Z]/g, (character) => character.toLowerCase());
}
