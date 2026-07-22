import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  Check,
  Copy,
  Eye,
  EyeOff,
  LoaderCircle,
  Pin,
  RefreshCw,
  Search,
  Variable,
  X,
} from "lucide-react";
import "./QuickPanel.css";
import {
  apiErrorMessage,
  copyText,
  getEnvironmentRevision,
  getEnvironmentSnapshot,
  getFavorites,
} from "./lib/api";
import {
  buildQuickRows,
  nextQuickSelection,
  quickCopyValue,
  quickDisplayValue,
  shouldRefreshQuick,
  type QuickSelectionKey,
} from "./lib/quick-panel";
import type {
  EnvironmentSnapshot,
  FavoriteKey,
} from "./types";

const selectionKeys = new Set<QuickSelectionKey>([
  "ArrowDown",
  "ArrowUp",
  "Home",
  "End",
  "Enter",
  "Escape",
]);

interface QuickNotice {
  message: string;
  tone: "success" | "warning";
}

export function QuickPanel() {
  const [snapshot, setSnapshot] = useState<EnvironmentSnapshot | null>(null);
  const [favorites, setFavorites] = useState<FavoriteKey[]>([]);
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(-1);
  const [revealedRows, setRevealedRows] = useState<Set<string>>(() => new Set());
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [copyingRow, setCopyingRow] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<QuickNotice | null>(null);
  const mountedRef = useRef(false);
  const requestGeneration = useRef(0);
  const revisionGeneration = useRef(0);
  const snapshotRef = useRef<EnvironmentSnapshot | null>(null);
  const busyRef = useRef(false);
  const rowRefs = useRef<Array<HTMLDivElement | null>>([]);

  const rows = useMemo(
    () => buildQuickRows(snapshot?.effectiveVariables ?? [], favorites, query),
    [snapshot, favorites, query],
  );

  const refresh = useCallback(async (showLoading: boolean) => {
    const generation = ++requestGeneration.current;
    revisionGeneration.current += 1;
    busyRef.current = true;
    if (showLoading) setLoading(true);
    else setRefreshing(true);
    setError(null);
    setNotice(null);

    const [snapshotResult, favoritesResult] = await Promise.allSettled([
      getEnvironmentSnapshot(),
      getFavorites(),
    ]);

    if (!mountedRef.current || generation !== requestGeneration.current) return;

    if (snapshotResult.status === "fulfilled") {
      snapshotRef.current = snapshotResult.value;
      setSnapshot(snapshotResult.value);
    } else {
      setError(apiErrorMessage(snapshotResult.reason));
    }

    if (favoritesResult.status === "fulfilled") {
      setFavorites(favoritesResult.value);
    } else if (snapshotResult.status === "fulfilled") {
      setNotice({
        message: `Favorites unavailable: ${apiErrorMessage(favoritesResult.reason)}`,
        tone: "warning",
      });
    }

    busyRef.current = false;
    setLoading(false);
    setRefreshing(false);
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    void refresh(true);
    return () => {
      mountedRef.current = false;
      requestGeneration.current += 1;
      revisionGeneration.current += 1;
    };
  }, [refresh]);

  useEffect(() => {
    const refreshOnFocus = () => void refresh(false);
    window.addEventListener("focus", refreshOnFocus);
    return () => window.removeEventListener("focus", refreshOnFocus);
  }, [refresh]);

  useEffect(() => {
    const pollRevision = async () => {
      const current = snapshotRef.current;
      if (!current || busyRef.current) return;
      const generation = ++revisionGeneration.current;
      try {
        const observed = await getEnvironmentRevision();
        if (
          mountedRef.current &&
          generation === revisionGeneration.current &&
          shouldRefreshQuick(current.revision, observed, "poll")
        ) {
          await refresh(false);
        }
      } catch (nextError) {
        if (mountedRef.current && generation === revisionGeneration.current) {
          setNotice({
            message: `Refresh check failed: ${apiErrorMessage(nextError)}`,
            tone: "warning",
          });
        }
      }
    };
    const interval = window.setInterval(() => void pollRevision(), 2000);
    return () => window.clearInterval(interval);
  }, [refresh]);

  useEffect(() => {
    setSelectedIndex((current) => {
      if (rows.length === 0 || current < 0) return -1;
      return Math.min(current, rows.length - 1);
    });
  }, [rows.length, query]);

  useEffect(() => {
    if (selectedIndex >= 0) {
      rowRefs.current[selectedIndex]?.scrollIntoView({ block: "nearest" });
    }
  }, [selectedIndex]);

  const copyRow = useCallback(async (index: number) => {
    const row = rows[index];
    if (!row || copyingRow !== null) return;
    setCopyingRow(row.id);
    setError(null);
    setNotice(null);
    try {
      await copyText(quickCopyValue(row));
      if (mountedRef.current) {
        setNotice({ message: "Value copied to clipboard.", tone: "success" });
      }
    } catch (nextError) {
      if (mountedRef.current) setError(apiErrorMessage(nextError));
    } finally {
      if (mountedRef.current) setCopyingRow(null);
    }
  }, [copyingRow, rows]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      if (!selectionKeys.has(event.key as QuickSelectionKey)) return;
      const target = event.target;
      if (target instanceof Element && target.closest("button")) return;

      const key = event.key as QuickSelectionKey;
      if (key === "Escape") {
        if (!query && revealedRows.size === 0 && selectedIndex < 0) return;
        event.preventDefault();
        setQuery("");
        setRevealedRows(new Set());
        setSelectedIndex(-1);
        return;
      }
      if (key === "Enter") {
        if (selectedIndex < 0 || selectedIndex >= rows.length) return;
        event.preventDefault();
        void copyRow(selectedIndex);
        return;
      }
      event.preventDefault();
      setSelectedIndex((current) => nextQuickSelection(current, key, rows.length));
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [copyRow, query, revealedRows.size, rows.length, selectedIndex]);

  const clearSearch = () => {
    setQuery("");
    setSelectedIndex(-1);
  };

  return (
    <main className="quick-panel">
      <header className="quick-header">
        <div className="quick-brand-mark"><Variable size={15} /></div>
        <div className="quick-brand-copy">
          <strong>EnvManager</strong>
          <span>{snapshot ? `${rows.length} effective variables` : "Quick access"}</span>
        </div>
        <button
          className="quick-icon-button"
          type="button"
          title="Refresh variables"
          aria-label="Refresh variables"
          disabled={refreshing || loading}
          onClick={() => void refresh(false)}
        >
          <RefreshCw className={refreshing ? "spin" : ""} size={16} />
        </button>
      </header>

      <div className="quick-search">
        <Search size={15} aria-hidden="true" />
        <input
          autoFocus
          type="search"
          value={query}
          placeholder="Search name or value"
          aria-label="Search effective variables"
          aria-controls="quick-variable-list"
          aria-activedescendant={selectedIndex >= 0 ? `quick-row-${selectedIndex}` : undefined}
          onChange={(event) => setQuery(event.target.value)}
        />
        {query && (
          <button
            type="button"
            title="Clear search"
            aria-label="Clear search"
            onClick={clearSearch}
          >
            <X size={14} />
          </button>
        )}
      </div>

      <section
        id="quick-variable-list"
        className="quick-list"
        role="listbox"
        aria-label="Effective environment variables"
      >
        {loading && !snapshot ? (
          <div className="quick-state"><LoaderCircle className="spin" size={20} /><span>Loading variables...</span></div>
        ) : !snapshot ? (
          <div className="quick-state error"><span>Variables could not be loaded.</span><button type="button" onClick={() => void refresh(true)}>Try again</button></div>
        ) : rows.length === 0 ? (
          <div className="quick-state"><Search size={20} /><span>No matching variables</span></div>
        ) : rows.map((row, index) => {
          const revealed = revealedRows.has(row.id);
          const isCopying = copyingRow === row.id;
          return (
            <div
              key={row.id}
              id={`quick-row-${index}`}
              ref={(element) => { rowRefs.current[index] = element; }}
              className={`quick-row${selectedIndex === index ? " selected" : ""}`}
              role="option"
              aria-selected={selectedIndex === index}
              onClick={() => setSelectedIndex(index)}
            >
              <div className="quick-row-content">
                <div className="quick-row-title">
                  <strong title={row.name}>{row.name}</strong>
                  <span className={`quick-source ${row.source}`}>{row.source}</span>
                  {row.isFavorite && <Pin className="quick-pin" size={13} aria-label="Favorite" />}
                </div>
                <code
                  className={row.isSensitive && !revealed ? "masked" : undefined}
                  title={row.isSensitive && !revealed ? "Sensitive value hidden" : row.value}
                >
                  {quickDisplayValue(row, revealed)}
                </code>
              </div>
              <div className="quick-row-actions">
                {row.isSensitive && (
                  <button
                    className="quick-icon-button compact"
                    type="button"
                    title={revealed ? "Hide value" : "Reveal value"}
                    aria-label={revealed ? `Hide ${row.name}` : `Reveal ${row.name}`}
                    onClick={(event) => {
                      event.stopPropagation();
                      setRevealedRows((current) => {
                        const next = new Set(current);
                        if (next.has(row.id)) next.delete(row.id);
                        else next.add(row.id);
                        return next;
                      });
                    }}
                  >
                    {revealed ? <EyeOff size={14} /> : <Eye size={14} />}
                  </button>
                )}
                <button
                  className="quick-icon-button compact"
                  type="button"
                  title="Copy value"
                  aria-label={`Copy ${row.name} value`}
                  disabled={copyingRow !== null}
                  onClick={(event) => {
                    event.stopPropagation();
                    void copyRow(index);
                  }}
                >
                  {isCopying ? <LoaderCircle className="spin" size={14} /> : <Copy size={14} />}
                </button>
              </div>
            </div>
          );
        })}
      </section>

      <footer
        className={`quick-footer${error ? " error" : notice ? ` ${notice.tone}` : ""}`}
      >
        {error ? (
          <><X size={12} /><span>{error}</span></>
        ) : notice ? (
          <>
            {notice.tone === "warning"
              ? <AlertCircle size={12} />
              : <Check size={12} />}
            <span>{notice.message}</span>
          </>
        ) : (
          <span>Enter to copy | Esc to clear</span>
        )}
      </footer>
    </main>
  );
}
