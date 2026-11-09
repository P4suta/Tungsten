export interface Saved {
  id: string;
  name: string;
  modified: number;
  captured: number;
  revision: string;
  bytes: number;
  title: string;
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export function savedEntries(value: unknown): Saved[] {
  if (!Array.isArray(value)) throw new Error("The saved file list is invalid.");
  return value.map((entry: unknown) => {
    if (
      !record(entry) ||
      typeof entry.id !== "string" ||
      typeof entry.name !== "string" ||
      typeof entry.revision !== "string" ||
      typeof entry.modified !== "number" ||
      typeof entry.bytes !== "number"
    )
      throw new Error("The saved file list is invalid.");
    return {
      id: entry.id,
      name: entry.name,
      revision: entry.revision,
      modified: entry.modified,
      captured: typeof entry.captured === "number" ? entry.captured : entry.modified,
      bytes: entry.bytes,
      title: typeof entry.title === "string" ? entry.title : "",
    };
  });
}
