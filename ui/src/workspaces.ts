export interface Workspace {
  readonly id: string;
  readonly title: string;
  readonly revision: string;
}

export interface NoteEntry {
  readonly name: string;
  readonly title: string;
}

export interface WorkspacePage {
  readonly workspaces: Workspace[];
  readonly next: string | null;
  readonly revision: string;
}

export interface WorkspaceNotes {
  readonly workspace: Workspace;
  readonly notes: NoteEntry[];
  readonly next: string | null;
  readonly revision: string;
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export function validWorkspaceId(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^[1-9][0-9]{0,18}$/.test(value) &&
    BigInt(value) <= 9223372036854775807n
  );
}

function revision(value: unknown): value is string {
  return typeof value === "string" && /^"[0-9a-f]{64}"$/.test(value);
}

function cursor(value: unknown): value is string | null {
  return value === null || validWorkspaceId(value);
}

export function workspace(value: unknown): Workspace {
  if (
    !record(value) ||
    !validWorkspaceId(value.id) ||
    typeof value.title !== "string" ||
    !value.title.trim() ||
    Array.from(value.title).length > 120 ||
    !revision(value.revision)
  )
    throw new Error("The workspace data is invalid.");
  return { id: value.id, title: value.title, revision: value.revision };
}

export function workspacePage(value: unknown): WorkspacePage {
  if (
    !record(value) ||
    !Array.isArray(value.workspaces) ||
    value.workspaces.length > 50 ||
    !cursor(value.next) ||
    !revision(value.revision)
  )
    throw new Error("The workspace list is invalid.");
  const workspaces = value.workspaces.map(workspace);
  if (new Set(workspaces.map((entry) => entry.id)).size !== workspaces.length)
    throw new Error("The workspace list repeats an identifier.");
  return { workspaces, next: value.next, revision: value.revision };
}

export function workspaceNotes(value: unknown): WorkspaceNotes {
  if (
    !record(value) ||
    !Array.isArray(value.notes) ||
    value.notes.length > 50 ||
    !cursor(value.next) ||
    !revision(value.revision)
  )
    throw new Error("The workspace notes are invalid.");
  const notes = value.notes.map((entry: unknown): NoteEntry => {
    if (
      !record(entry) ||
      typeof entry.name !== "string" ||
      !entry.name ||
      new TextEncoder().encode(entry.name).length > 240 ||
      typeof entry.title !== "string"
    )
      throw new Error("The workspace notes are invalid.");
    return { name: entry.name, title: entry.title };
  });
  if (new Set(notes.map((entry) => entry.name)).size !== notes.length)
    throw new Error("The workspace notes repeat an identity.");
  return {
    workspace: workspace(value.workspace),
    notes,
    next: value.next,
    revision: value.revision,
  };
}
