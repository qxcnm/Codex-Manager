export type CodexProjectLaunchAction = "start" | "resume";
export type CodexProjectLaunchTarget = "app" | "cli";

export interface CodexProjectSummary {
  path: string;
  name: string;
  addedAt: number;
  available: boolean;
}

export interface CodexProjectListResult {
  items: CodexProjectSummary[];
}

export interface CodexProjectAddResult {
  canceled: boolean;
  added: boolean;
  project: CodexProjectSummary | null;
}

export interface CodexProjectRemoveResult {
  removed: boolean;
}

export interface CodexProjectLaunchResult {
  path: string;
  action: CodexProjectLaunchAction;
  target: CodexProjectLaunchTarget;
  codexHome: string | null;
}
