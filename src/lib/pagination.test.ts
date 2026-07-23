import { describe, expect, it } from "vitest";
import { paginateItems } from "./pagination";

describe("paginateItems", () => {
  const items = Array.from({ length: 23 }, (_, index) => index + 1);

  it("returns the requested page and range", () => {
    expect(paginateItems(items, 2, 10)).toEqual({
      items: [11, 12, 13, 14, 15, 16, 17, 18, 19, 20],
      page: 2,
      pageCount: 3,
      pageSize: 10,
      start: 11,
      end: 20,
      total: 23,
    });
  });

  it("clamps pages after the item count shrinks", () => {
    const result = paginateItems(items.slice(0, 12), 4, 10);

    expect(result.page).toBe(2);
    expect(result.items).toEqual([11, 12]);
    expect(result.end).toBe(12);
  });

  it("reports an empty collection without creating page zero", () => {
    expect(paginateItems([], 1, 20)).toMatchObject({
      items: [],
      page: 1,
      pageCount: 1,
      start: 0,
      end: 0,
      total: 0,
    });
  });

  it("rejects invalid page sizes", () => {
    expect(() => paginateItems(items, 1, 0)).toThrow(RangeError);
  });
});
