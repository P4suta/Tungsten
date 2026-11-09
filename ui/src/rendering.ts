export const previewCharacters = 256 * 1024;
export const previewLines = 10_000;

export function withinPreviewBudget(source: string): boolean {
  if (source.length > previewCharacters) return false;
  let lines = 1;
  for (const character of source) {
    if (character === "\n" && ++lines > previewLines) return false;
  }
  return true;
}
