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

    const saved = await api.saveEnvironmentVariable(input);

    expectMutationResult(saved);
    expect(saved.snapshot.revision).not.toBe(before.revision);
    expect(findValue(saved.snapshot, "user", "JAVA_HOME")).toBe(input.value);

    const undone = await api.undoEnvironmentMutation(saved.undoBackupIds);

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

    await api.deleteEnvironmentVariable("user", "JAVA_HOME");

    expect(await api.getEnvironmentRevision()).not.toBe(before);
  });

  it("toggles favorites and reconciles variables that no longer exist", async () => {
    const api = await loadPreviewApi();
    const favorite: FavoriteKeyContract = { scope: "user", name: "JAVA_HOME" };

    expect(await api.getFavorites()).toEqual([]);
    expect(await api.toggleFavorite(favorite)).toEqual([favorite]);

    await api.deleteEnvironmentVariable("user", "JAVA_HOME");

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
    await api.undoEnvironmentMutation(["user-backup", "system-backup"]);

    expect(invoke).toHaveBeenNthCalledWith(1, "transfer_environment_variable", {
      input,
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "toggle_favorite", { favorite });
    expect(invoke).toHaveBeenNthCalledWith(3, "undo_environment_mutation", {
      backupIds: ["user-backup", "system-backup"],
    });
  });
});

async function loadPreviewApi() {
  const api = await import("./api");
  return api as typeof api & {
    getEnvironmentRevision(): Promise<string>;
    undoEnvironmentMutation(backupIds: string[]): Promise<MutationResultContract>;
    transferEnvironmentVariable(
      input: TransferVariableInputContract,
    ): Promise<MutationResultContract>;
    getFavorites(): Promise<FavoriteKeyContract[]>;
    toggleFavorite(favorite: FavoriteKeyContract): Promise<FavoriteKeyContract[]>;
    saveEnvironmentVariable(
      input: EnvironmentVariableInput,
    ): Promise<MutationResultContract>;
    deleteEnvironmentVariable(
      scope: EnvironmentScope,
      name: string,
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
