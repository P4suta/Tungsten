import { parser } from "@lezer/markdown";

export function normalizeHeadings(text: string): string {
  const edits: { from: number; to: number; text: string }[] = [];
  let previous = 0;
  parser.parse(text).iterate({
    enter(ref) {
      const heading = /^(ATX|Setext)Heading([1-6])$/.exec(ref.name);
      const marker = ref.node.getChild("HeaderMark");
      if (!heading || !marker) return;
      const level = previous === 0 ? 1 : Math.min(Math.max(2, Number(heading[2])), previous + 1);
      previous = level;
      const replacement =
        heading[1] === "ATX"
          ? "#".repeat(level)
          : (level === 1 ? "=" : "-").repeat(marker.to - marker.from);
      if (text.slice(marker.from, marker.to) !== replacement)
        edits.push({ from: marker.from, to: marker.to, text: replacement });
    },
  });
  const parts: string[] = [];
  let offset = 0;
  for (const edit of edits) {
    parts.push(text.slice(offset, edit.from), edit.text);
    offset = edit.to;
  }
  parts.push(text.slice(offset));
  return parts.join("");
}
