import { useEffect, useState } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";
import {
  DEFAULT_PAGE_SIZE,
  PAGE_SIZE_OPTIONS,
  paginateItems,
  type PageSlice,
} from "../lib/pagination";

export function useTablePagination<T>(items: T[], resetKey: string) {
  const [requestedPage, setRequestedPage] = useState(1);
  const [pageSize, setPageSize] = useState(DEFAULT_PAGE_SIZE);
  const pagination = paginateItems(items, requestedPage, pageSize);

  useEffect(() => {
    setRequestedPage(1);
  }, [resetKey]);

  useEffect(() => {
    if (requestedPage !== pagination.page) setRequestedPage(pagination.page);
  }, [pagination.page, requestedPage]);

  return {
    ...pagination,
    setPage: setRequestedPage,
    setPageSize: (nextPageSize: number) => {
      setPageSize(nextPageSize);
      setRequestedPage(1);
    },
  };
}

export function TablePagination({
  page,
  pageCount,
  pageSize,
  start,
  end,
  total,
  onPageChange,
  onPageSizeChange,
}: Omit<PageSlice<unknown>, "items"> & {
  onPageChange: (page: number) => void;
  onPageSizeChange: (pageSize: number) => void;
}) {
  return (
    <footer className="table-pagination">
      <label className="page-size-control">
        <span>Rows per page</span>
        <select
          aria-label="Rows per page"
          onChange={(event) => onPageSizeChange(Number(event.target.value))}
          value={pageSize}
        >
          {PAGE_SIZE_OPTIONS.map((option) => (
            <option key={option} value={option}>{option}</option>
          ))}
        </select>
      </label>
      <span className="page-range">{start}-{end} of {total}</span>
      <span aria-live="polite" className="page-number">Page {page} of {pageCount}</span>
      <div className="page-actions">
        <button
          aria-label="Previous page"
          disabled={page <= 1}
          onClick={() => onPageChange(page - 1)}
          title="Previous page"
          type="button"
        >
          <ChevronLeft size={16} />
        </button>
        <button
          aria-label="Next page"
          disabled={page >= pageCount}
          onClick={() => onPageChange(page + 1)}
          title="Next page"
          type="button"
        >
          <ChevronRight size={16} />
        </button>
      </div>
    </footer>
  );
}
