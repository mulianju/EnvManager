import { describe, expect, it } from "vitest";
import type {
  EnvironmentScope,
  EnvironmentVariable,
  ImportPreview,
} from "../types";
import {
  createExportRequest,
  createImportRequest,
  defaultExportFileName,
  deriveTransferFileFormat,
  importConfirmationMessage,
  previewWritesSystem,
  summarizeImportPreview,
} from "./import-export-workflow";

describe("import and export workflow", () => {
  it.each([
    ["C:\\backup\\environment.json", "json"],
    ["C:\\backup\\environment.ENV", "dotEnv"],
    ["C:\\backup\\environment.reg", "registry"],
    ["C:\\backup\\environment.txt", null],
  ] as const)("derives the transfer format from %s", (path, expected) => {
    expect(deriveTransferFileFormat(path, null)).toBe(expected);
  });

  it("uses an explicitly selected format instead of the file extension", () => {
    expect(
      deriveTransferFileFormat("C:\\backup\\environment.json", "registry"),
    ).toBe("registry");
  });

  it("requires a target scope only for .env imports", () => {
    expect(
      createImportRequest("C:\\backup\\environment.env", "dotEnv", null),
    ).toBeNull();
    expect(
      createImportRequest("C:\\backup\\environment.env", "dotEnv", "system"),
    ).toEqual({
      path: "C:\\backup\\environment.env",
      format: "dotEnv",
      defaultScope: "system",
    });
  });

  it("preserves scopes from JSON and registry files", () => {
    expect(
      createImportRequest("C:\\backup\\environment.json", null, "user"),
    ).toEqual({
      path: "C:\\backup\\environment.json",
      format: "json",
      defaultScope: null,
    });
    expect(
      createImportRequest("C:\\backup\\environment.reg", null, "system"),
    ).toEqual({
      path: "C:\\backup\\environment.reg",
      format: "registry",
      defaultScope: null,
    });
    expect(createImportRequest("C:\\backup\\environment.txt", null, null)).toBeNull();
  });

  it("counts create, update, and unchanged preview items without dropping detail", () => {
    const preview = importPreview();

    expect(summarizeImportPreview(preview)).toEqual({
      create: 1,
      update: 1,
      unchanged: 1,
      total: 3,
      items: preview.items,
    });
  });

  it("states the impact of overwrite and skip-existing strategies", () => {
    const preview = importPreview();
    const overwrite = importConfirmationMessage(preview, "overwrite");
    const skipExisting = importConfirmationMessage(preview, "skipExisting");

    expect(overwrite).toMatch(/create 1/i);
    expect(overwrite).toMatch(/overwrite 1/i);
    expect(overwrite).toMatch(/1 unchanged/i);
    expect(skipExisting).toMatch(/create 1/i);
    expect(skipExisting).toMatch(/skip 1/i);
    expect(skipExisting).toMatch(/1 unchanged/i);
  });

  it("builds export requests and requires one scope for .env", () => {
    expect(createExportRequest("C:\\backup\\all.json", "json", null)).toEqual({
      path: "C:\\backup\\all.json",
      format: "json",
      scope: null,
    });
    expect(createExportRequest("C:\\backup\\system.reg", "registry", "system"))
      .toEqual({
        path: "C:\\backup\\system.reg",
        format: "registry",
        scope: "system",
      });
    expect(createExportRequest("C:\\backup\\all.env", "dotEnv", null)).toBeNull();
    expect(createExportRequest("C:\\backup\\user.env", "dotEnv", "user")).toEqual({
      path: "C:\\backup\\user.env",
      format: "dotEnv",
      scope: "user",
    });
  });

  it.each([
    ["json", null, "environment-all.json"],
    ["dotEnv", "user", "environment-user.env"],
    ["registry", "system", "environment-system.reg"],
  ] as const)(
    "creates a default filename with the selected format extension",
    (format, scope, expected) => {
      expect(defaultExportFileName(format, scope)).toBe(expected);
    },
  );

  it.each([
    ["create", "overwrite", true],
    ["create", "skipExisting", true],
    ["create", null, true],
    ["update", "overwrite", true],
    ["update", "skipExisting", false],
    ["update", null, false],
    ["unchanged", "overwrite", false],
    ["unchanged", "skipExisting", false],
    ["unchanged", null, false],
  ] as const)(
    "reports whether a System %s writes with %s",
    (action, strategy, expected) => {
      const item = previewItem(
        variable("SYSTEM_VALUE", "new", "system"),
        action === "create" ? null : variable("SYSTEM_VALUE", "old", "system"),
        action,
      );

      expect(previewWritesSystem(previewWithItems(item), strategy)).toBe(expected);
    },
  );

  it("only blocks mixed previews when the selected strategy writes System updates", () => {
    const preview = previewWithItems(
      previewItem(variable("USER_NEW", "new"), null, "create"),
      previewItem(
        variable("SYSTEM_UPDATE", "new", "system"),
        variable("SYSTEM_UPDATE", "old", "system"),
        "update",
      ),
    );

    expect(previewWritesSystem(preview, "skipExisting")).toBe(false);
    expect(previewWritesSystem(preview, "overwrite")).toBe(true);
    expect(previewWritesSystem(preview, null)).toBe(false);
  });

  it("does not report writes before a preview or for User-only changes", () => {
    const preview = previewWithItems(
      previewItem(variable("USER_NEW", "new"), null, "create"),
    );

    expect(previewWritesSystem(null, "overwrite")).toBe(false);
    expect(previewWritesSystem(preview, "overwrite")).toBe(false);
  });
});

function importPreview(): ImportPreview {
  return {
    token: "preview-token",
    environmentRevision: "preview-revision",
    items: [
      previewItem(variable("NEW_VALUE", "new"), null, "create"),
      previewItem(
        variable("JAVA_HOME", "C:\\Java\\new"),
        variable("JAVA_HOME", "C:\\Java\\old"),
        "update",
      ),
      previewItem(
        variable("SAME_VALUE", "same"),
        variable("SAME_VALUE", "same"),
        "unchanged",
      ),
    ],
  };
}

function previewWithItems(
  ...items: ImportPreview["items"]
): ImportPreview {
  return {
    token: "preview-token",
    environmentRevision: "preview-revision",
    items,
  };
}

function previewItem(
  variableValue: EnvironmentVariable,
  existing: EnvironmentVariable | null,
  action: "create" | "update" | "unchanged",
): ImportPreview["items"][number] {
  return { variable: variableValue, existing, action };
}

function variable(
  name: string,
  value: string,
  scope: EnvironmentScope = "user",
): EnvironmentVariable {
  return { name, value, valueType: "string", scope };
}
