import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  EnvironmentScope,
  EnvironmentSnapshot,
  EnvironmentVariableInput,
  ImportFileRequest,
} from "../types";

interface MutationResultContract {
  snapshot: EnvironmentSnapshot;
  undoBackupIds: string[];
}

interface FavoriteKeyContract {
  scope: EnvironmentScope;
  name: string;
}

interface TransferVariableInputContract {
  sourceScope: EnvironmentScope;
  targetScope: EnvironmentScope;
  name: string;
  mode: "copy" | "move";
  overwrite: boolean;
}

describe("browser preview API contracts", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.stubGlobal("window", {});
  });

  afterEach(() => {
    vi.doUnmock("@tauri-apps/api/core");
    vi.doUnmock("@tauri-apps/plugin-dialog");
    vi.unstubAllGlobals();
  });

  it("returns a mutation receipt and can undo a save", async () => {
    const api = await loadPreviewApi();
    const before = await api.getEnvironmentSnapshot();
    const input: EnvironmentVariableInput = {
      originalName: "JAVA_HOME",
      name: "JAVA_HOME",
      value: "C:\\Program Files\\Java\\jdk-25",
      valueType: "string",
      scope: "user",
    };

    const saved = await api.saveEnvironmentVariable(input, before.revision);

    expectMutationResult(saved);
    expect(saved.snapshot.revision).not.toBe(before.revision);
    expect(findValue(saved.snapshot, "user", "JAVA_HOME")).toBe(input.value);

    const undone = await api.undoEnvironmentMutation(
      saved.undoBackupIds,
      saved.snapshot.revision,
    );

    expectMutationResult(undone);
    expect(findValue(undone.snapshot, "user", "JAVA_HOME")).toBe(
      findValue(before, "user", "JAVA_HOME"),
    );
  });

  it("copies and moves variables between scopes", async () => {
    const api = await loadPreviewApi();
    const copied = await api.transferEnvironmentVariable({
      sourceScope: "system",
      targetScope: "user",
      name: "ComSpec",
      mode: "copy",
      overwrite: false,
    });

    expectMutationResult(copied);
    expect(findValue(copied.snapshot, "system", "ComSpec")).toBeTruthy();
    expect(findValue(copied.snapshot, "user", "ComSpec")).toBeTruthy();

    await api.restartElevated();
    const moved = await api.transferEnvironmentVariable({
      sourceScope: "user",
      targetScope: "system",
      name: "JAVA_HOME",
      mode: "move",
      overwrite: false,
    });

    expectMutationResult(moved);
    expect(findValue(moved.snapshot, "user", "JAVA_HOME")).toBeUndefined();
    expect(findValue(moved.snapshot, "system", "JAVA_HOME")).toBeTruthy();
  });

  it("returns a stable revision that changes after mutation", async () => {
    const api = await loadPreviewApi();
    const before = await api.getEnvironmentRevision();
    const snapshot = await api.getEnvironmentSnapshot();

    expect(typeof before).toBe("string");
    expect(before).toBe(snapshot.revision);

    await api.deleteEnvironmentVariable("user", "JAVA_HOME", snapshot.revision);

    expect(await api.getEnvironmentRevision()).not.toBe(before);
  });

  it("toggles favorites and reconciles variables that no longer exist", async () => {
    const api = await loadPreviewApi();
    const favorite: FavoriteKeyContract = { scope: "user", name: "JAVA_HOME" };

    expect(await api.getFavorites()).toEqual([]);
    expect(await api.toggleFavorite(favorite)).toEqual([favorite]);

    const snapshot = await api.getEnvironmentSnapshot();
    await api.deleteEnvironmentVariable("user", "JAVA_HOME", snapshot.revision);

    expect(await api.getFavorites()).toEqual([]);
  });

  it("passes the previewed environment revision when applying an import", async () => {
    const invoke = vi.fn().mockResolvedValue({});
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    vi.doMock("@tauri-apps/api/core", () => ({ invoke }));
    const api = await import("./api");
    const applyImport = api.applyEnvironmentImport as unknown as (
      request: ImportFileRequest,
      strategy: "overwrite",
      expectedToken: string,
      expectedRevision: string,
    ) => Promise<unknown>;
    const request: ImportFileRequest = {
      path: "C:\\temp\\variables.env",
      format: "dotEnv",
      defaultScope: "user",
    };

    await applyImport(request, "overwrite", "preview-token", "preview-revision");

    expect(invoke).toHaveBeenCalledWith("apply_environment_import", {
      request,
      strategy: "overwrite",
      expectedToken: "preview-token",
      expectedRevision: "preview-revision",
    });
  });

  it("passes transfer, favorite, and undo receipts through typed Tauri payloads", async () => {
    const invoke = vi.fn().mockResolvedValue({});
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    vi.doMock("@tauri-apps/api/core", () => ({ invoke }));
    const api = await import("./api");
    const input: TransferVariableInputContract = {
      sourceScope: "user",
      targetScope: "system",
      name: "JAVA_HOME",
      mode: "move",
      overwrite: true,
    };
    const favorite: FavoriteKeyContract = { scope: "user", name: "JAVA_HOME" };

    await api.transferEnvironmentVariable(input);
    await api.toggleFavorite(favorite);
    await api.undoEnvironmentMutation(
      ["user-backup", "system-backup"],
      "expected-revision",
    );

    expect(invoke).toHaveBeenNthCalledWith(1, "transfer_environment_variable", {
      input,
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "toggle_favorite", { favorite });
    expect(invoke).toHaveBeenNthCalledWith(3, "undo_environment_mutation", {
      backupIds: ["user-backup", "system-backup"],
      expectedRevision: "expected-revision",
    });
  });

  it("passes editor revisions through save and delete commands", async () => {
    const invoke = vi.fn().mockResolvedValue({});
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    vi.doMock("@tauri-apps/api/core", () => ({ invoke }));
    const api = await import("./api");
    const input: EnvironmentVariableInput = {
      originalName: "JAVA_HOME",
      name: "JAVA_HOME",
      value: "C:\\Java",
      valueType: "string",
      scope: "user",
    };

    await api.saveEnvironmentVariable(input, "editor-revision");
    await api.deleteEnvironmentVariable("user", "JAVA_HOME", "editor-revision");

    expect(invoke).toHaveBeenNthCalledWith(1, "save_environment_variable", {
      input,
      expectedRevision: "editor-revision",
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "delete_environment_variable", {
      scope: "user",
      name: "JAVA_HOME",
      expectedRevision: "editor-revision",
    });
    expect(api.apiErrorCode({ code: "variableAlreadyExists", message: "collision" }))
      .toBe("variableAlreadyExists");
  });

  it("opens the native folder picker with a single-directory payload and preserves cancellation", async () => {
    const open = vi.fn()
      .mockResolvedValueOnce("C:\\Program Files\\Java")
      .mockResolvedValueOnce(null);
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    vi.doMock("@tauri-apps/plugin-dialog", () => ({ open, save: vi.fn() }));
    const api = await import("./api");

    await expect(api.pickEnvironmentFolder()).resolves.toBe("C:\\Program Files\\Java");
    await expect(api.pickEnvironmentFolder()).resolves.toBeNull();
    expect(open).toHaveBeenNthCalledWith(1, {
      directory: true,
      multiple: false,
    });
    expect(open).toHaveBeenNthCalledWith(2, {
      directory: true,
      multiple: false,
    });
  });

  it("opens the native import picker with supported formats and preserves cancellation", async () => {
    const open = vi.fn()
      .mockResolvedValueOnce("C:\\backup\\environment.json")
      .mockResolvedValueOnce(null);
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    vi.doMock("@tauri-apps/plugin-dialog", () => ({ open, save: vi.fn() }));
    const api = await import("./api");

    await expect(api.pickImportFile()).resolves.toBe("C:\\backup\\environment.json");
    await expect(api.pickImportFile()).resolves.toBeNull();
    expect(open).toHaveBeenNthCalledWith(1, {
      multiple: false,
      filters: [
        { name: "Environment files", extensions: ["json", "env", "reg"] },
      ],
    });
    expect(open).toHaveBeenNthCalledWith(2, {
      multiple: false,
      filters: [
        { name: "Environment files", extensions: ["json", "env", "reg"] },
      ],
    });
  });

  it("maps export formats to native save filters and preserves cancellation", async () => {
    const save = vi.fn()
      .mockResolvedValueOnce("C:\\backup\\environment-all.json")
      .mockResolvedValueOnce("C:\\backup\\environment-user.env")
      .mockResolvedValueOnce("C:\\backup\\environment-system.reg")
      .mockResolvedValueOnce(null);
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    vi.doMock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save }));
    const api = await import("./api");

    await expect(api.pickExportFile("json", "environment-all.json"))
      .resolves.toBe("C:\\backup\\environment-all.json");
    await expect(api.pickExportFile("dotEnv", "environment-user.env"))
      .resolves.toBe("C:\\backup\\environment-user.env");
    await expect(api.pickExportFile("registry", "environment-system.reg"))
      .resolves.toBe("C:\\backup\\environment-system.reg");
    await expect(api.pickExportFile("json", "environment-all.json"))
      .resolves.toBeNull();

    expect(save).toHaveBeenNthCalledWith(1, {
      defaultPath: "environment-all.json",
      filters: [{ name: "Environment file", extensions: ["json"] }],
    });
    expect(save).toHaveBeenNthCalledWith(2, {
      defaultPath: "environment-user.env",
      filters: [{ name: "Environment file", extensions: ["env"] }],
    });
    expect(save).toHaveBeenNthCalledWith(3, {
      defaultPath: "environment-system.reg",
      filters: [{ name: "Environment file", extensions: ["reg"] }],
    });
    expect(save).toHaveBeenNthCalledWith(4, {
      defaultPath: "environment-all.json",
      filters: [{ name: "Environment file", extensions: ["json"] }],
    });
  });

  it("rejects stale save delete and undo receipts in browser preview", async () => {
    const api = await loadPreviewApi();
    const snapshot = await api.getEnvironmentSnapshot();
    const input: EnvironmentVariableInput = {
      originalName: "JAVA_HOME",
      name: "JAVA_HOME",
      value: "changed",
      valueType: "string",
      scope: "user",
    };

    await expect(api.saveEnvironmentVariable(input, "stale")).rejects.toMatchObject({
      code: "environmentChanged",
    });
    await expect(
      api.deleteEnvironmentVariable("user", "JAVA_HOME", "stale"),
    ).rejects.toMatchObject({ code: "environmentChanged" });
    await expect(
      api.undoEnvironmentMutation(["missing"], "stale"),
    ).rejects.toMatchObject({ code: "environmentChanged" });
    expect(await api.getEnvironmentSnapshot()).toEqual(snapshot);
  });
});

async function loadPreviewApi() {
  const api = await import("./api");
  return api as typeof api & {
    getEnvironmentRevision(): Promise<string>;
    undoEnvironmentMutation(
      backupIds: string[],
      expectedRevision: string,
    ): Promise<MutationResultContract>;
    transferEnvironmentVariable(
      input: TransferVariableInputContract,
    ): Promise<MutationResultContract>;
    getFavorites(): Promise<FavoriteKeyContract[]>;
    toggleFavorite(favorite: FavoriteKeyContract): Promise<FavoriteKeyContract[]>;
    saveEnvironmentVariable(
      input: EnvironmentVariableInput,
      expectedRevision: string,
    ): Promise<MutationResultContract>;
    deleteEnvironmentVariable(
      scope: EnvironmentScope,
      name: string,
      expectedRevision: string,
    ): Promise<MutationResultContract>;
  };
}

function expectMutationResult(result: MutationResultContract) {
  expect(result).toEqual({
    snapshot: expect.objectContaining({ revision: expect.any(String) }),
    undoBackupIds: expect.any(Array),
  });
  expect(result.undoBackupIds.length).toBeGreaterThan(0);
}

function findValue(
  snapshot: EnvironmentSnapshot,
  scope: EnvironmentScope,
  name: string,
): string | undefined {
  const variables = scope === "user" ? snapshot.userVariables : snapshot.systemVariables;
  return variables.find((variable) => variable.name === name)?.value;
}
