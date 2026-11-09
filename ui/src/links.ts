import { parser } from "@lezer/markdown";
import type { SlashRange } from "./slash.js";

export function noteLink(source: string, target: string, title: string): string {
  const from = source.split("/");
  from.pop();
  const to = target.split("/");
  while (from.length && to.length && from[0] === to[0]) {
    from.shift();
    to.shift();
  }
  return `[${title.replace(/[\\[\]]/g, "\\$&")}](${"../".repeat(from.length)}${to.map(encodeURIComponent).join("/")})`;
}

export function noteLinkRange(text: string, start: number, end: number): SlashRange | null {
  if (start !== end) return null;
  const tree = parser.parse(text);
  for (
    let node: typeof tree.topNode | null = tree.resolveInner(start, -1);
    node;
    node = node.parent
  )
    if (["FencedCode", "CodeBlock", "InlineCode", "Image"].includes(node.name)) return null;
  const open = text.lastIndexOf("[", start - 1);
  if (open < 0 || text[open - 1] === "!") return null;
  let escapes = 0;
  for (let index = open - 1; index >= 0 && text[index] === "\\"; index--) escapes++;
  const query = text.slice(open + 1, start);
  if (escapes % 2 || /[[\]\n]/.test(query)) return null;
  const after = text.slice(start);
  const suffix = /^[^[\]\n]*\]\(\)/.exec(after)?.[0];
  if (!suffix && after && !after.startsWith("\n")) return null;
  return { start: open, end: start + (suffix?.length ?? 0), query: query.toLocaleLowerCase() };
}
