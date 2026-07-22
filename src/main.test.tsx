import { describe, expect, it, vi } from "vitest";

vi.mock("react-dom/client", () => ({
  default: {
    createRoot: () => ({ render: vi.fn() }),
  },
}));

vi.stubGlobal("document", {
  getElementById: () => ({}),
});

describe("root component routing", () => {
  it("selects QuickPanel only for the explicit quick mode query", async () => {
    const { selectRootComponent } = await import("./main");

    expect(selectRootComponent("?mode=quick").name).toBe("QuickPanel");
    expect(selectRootComponent("?mode=QUICK").name).not.toBe("QuickPanel");
    expect(selectRootComponent("?mode=main").name).not.toBe("QuickPanel");
    expect(selectRootComponent("").name).not.toBe("QuickPanel");
  });
});
