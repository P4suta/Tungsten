import { diffLines } from "diff";

export interface VersionDifference {
  added: number;
  removed: number;
  text: string;
  complete: boolean;
}

export function versionDifference(
  before: string,
  after: string,
  maxEdits = 2000,
): VersionDifference | null {
  if (before.length + after.length > 256 * 1024) return null;
  const changes = diffLines(before, after, { maxEditLength: maxEdits, timeout: 40 });
  if (!changes) return null;
  let added = 0,
    removed = 0,
    count = 0,
    size = 0;
  const output: string[] = [];
  let complete = true;
  for (const change of changes) {
    if (change.added) added += change.count;
    if (change.removed) removed += change.count;
    const lines = change.value.replace(/\n$/, "").split("\n");
    const prefix = change.added ? "+ " : change.removed ? "- " : "  ";
    const context =
      !change.added && !change.removed && lines.length > 6
        ? [...lines.slice(0, 2), `… ${lines.length - 4} unchanged lines …`, ...lines.slice(-2)]
        : lines;
    for (const line of context) {
      if (count++ >= 200 || size + line.length > 16 * 1024) {
        complete = false;
        continue;
      }
      output.push(prefix + line);
      size += line.length;
    }
  }
  return { added, removed, text: output.join("\n"), complete };
}
