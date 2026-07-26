import type {
  CodexProjectAddResult,
  CodexProjectLaunchAction,
  CodexProjectLaunchResult,
  CodexProjectLaunchTarget,
  CodexProjectListResult,
  CodexProjectRemoveResult,
  CodexProjectSummary,
} from "@/types";
import { invoke } from "./transport";

export const CODEX_PROJECTS_QUERY_KEY = ["codex-projects", "desktop"] as const;

function asObject(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function asString(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function asBoolean(value: unknown, fallback = false): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function asNumber(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function normalizeProject(value: unknown): CodexProjectSummary | null {
  const source = asObject(value);
  const path = asString(source.path);
  if (!path) return null;
  return {
    path,
    name: asString(source.name) || path,
    addedAt: asNumber(source.addedAt ?? source.added_at),
    available: asBoolean(source.available),
  };
}

export function normalizeCodexProjectList(
  payload: unknown,
): CodexProjectListResult {
  const source = asObject(payload);
  return {
    items: (Array.isArray(source.items) ? source.items : [])
      .map(normalizeProject)
      .filter((item): item is CodexProjectSummary => Boolean(item)),
  };
}

export function normalizeCodexProjectAddResult(
  payload: unknown,
): CodexProjectAddResult {
  const source = asObject(payload);
  return {
    canceled: asBoolean(source.canceled),
    added: asBoolean(source.added),
    project: normalizeProject(source.project),
  };
}

export function normalizeCodexProjectRemoveResult(
  payload: unknown,
): CodexProjectRemoveResult {
  return { removed: asBoolean(asObject(payload).removed) };
}

function normalizeLaunchAction(value: unknown): CodexProjectLaunchAction {
  return asString(value) === "resume" ? "resume" : "start";
}

function normalizeLaunchTarget(value: unknown): CodexProjectLaunchTarget {
  return asString(value) === "app" ? "app" : "cli";
}

export function normalizeCodexProjectLaunchResult(
  payload: unknown,
): CodexProjectLaunchResult {
  const source = asObject(payload);
  return {
    path: asString(source.path),
    action: normalizeLaunchAction(source.action),
    target: normalizeLaunchTarget(source.target),
    codexHome: asString(source.codexHome ?? source.codex_home) || null,
  };
}

export const codexProjectsClient = {
  async list(): Promise<CodexProjectListResult> {
    return normalizeCodexProjectList(
      await invoke<unknown>("app_codex_projects_list"),
    );
  },

  async add(): Promise<CodexProjectAddResult> {
    return normalizeCodexProjectAddResult(
      await invoke<unknown>("app_codex_project_add"),
    );
  },

  async remove(path: string): Promise<CodexProjectRemoveResult> {
    return normalizeCodexProjectRemoveResult(
      await invoke<unknown>("app_codex_project_remove", { path }),
    );
  },

  async launch(params: {
    path: string;
    action: CodexProjectLaunchAction;
  }): Promise<CodexProjectLaunchResult> {
    return normalizeCodexProjectLaunchResult(
      await invoke<unknown>("app_codex_project_launch", params),
    );
  },
};
