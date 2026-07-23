export const DEFAULT_PAGE_SIZE = 20;
export const PAGE_SIZE_OPTIONS = [10, 20, 50] as const;

export interface PageSlice<T> {
  items: T[];
  page: number;
  pageCount: number;
  pageSize: number;
  start: number;
  end: number;
  total: number;
}

export function paginateItems<T>(
  items: T[],
  requestedPage: number,
  pageSize: number,
): PageSlice<T> {
  if (!Number.isInteger(pageSize) || pageSize <= 0) {
    throw new RangeError("Page size must be a positive integer.");
  }

  const total = items.length;
  const pageCount = Math.max(1, Math.ceil(total / pageSize));
  const page = Math.min(Math.max(1, Math.trunc(requestedPage) || 1), pageCount);
  const offset = (page - 1) * pageSize;
  const end = Math.min(offset + pageSize, total);

  return {
    items: items.slice(offset, end),
    page,
    pageCount,
    pageSize,
    start: total === 0 ? 0 : offset + 1,
    end,
    total,
  };
}
