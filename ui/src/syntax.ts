import { parser } from "@lezer/markdown";
import highlight from "highlight.js/lib/common";
import { withinPreviewBudget } from "./rendering.ts";

export interface CodeHighlight {
  from: number;
  to: number;
  className: string;
}

export async function codeHighlights(source: string): Promise<CodeHighlight[]> {
  if (!withinPreviewBudget(source)) return [];
  const tree = parser.parse(source);
  let uncommon = false;
  tree.iterate({
    enter(ref) {
      if (ref.name !== "FencedCode") return;
      const info = ref.node.getChild("CodeInfo");
      const language = info ? source.slice(info.from, info.to).trim().split(/\s/)[0] : undefined;
      if (language && !highlight.getLanguage(language)) uncommon = true;
    },
  });
  if (uncommon) await import("highlight.js");
  const ranges: CodeHighlight[] = [];
  tree.iterate({
    enter(ref) {
      if (ref.name !== "FencedCode") return;
      const info = ref.node.getChild("CodeInfo");
      const language = info ? source.slice(info.from, info.to).trim().split(/\s/)[0] : undefined;
      const parts = ref.node.getChildren("CodeText");
      if (!language || !highlight.getLanguage(language) || !parts.length) return;
      const code = parts.map((part) => source.slice(part.from, part.to)).join("");
      const template = document.createElement("template");
      template.innerHTML = highlight.highlight(code, { language, ignoreIllegals: true }).value;
      if (template.content.textContent !== code) return;
      let offset = 0;
      const tokens: CodeHighlight[] = [];
      function visit(node: Node, classes: string[]): void {
        if (node.nodeType === Node.TEXT_NODE) {
          const end = offset + (node.textContent?.length ?? 0);
          if (classes.some((name) => name.startsWith("hljs-")) && end > offset)
            tokens.push({ from: offset, to: end, className: classes.join(" ") });
          offset = end;
        } else {
          const names =
            node instanceof HTMLElement
              ? Array.from(node.classList).filter((name) => /^[a-z][a-z0-9_-]*$/.test(name))
              : [];
          for (const child of node.childNodes) visit(child, [...classes, ...names]);
        }
      }
      visit(template.content, []);
      let base = 0;
      let firstToken = 0;
      for (const part of parts) {
        const end = base + part.to - part.from;
        while (firstToken < tokens.length && (tokens[firstToken]?.to ?? 0) <= base) firstToken++;
        for (let index = firstToken; index < tokens.length; index++) {
          const token = tokens[index];
          if (!token || token.from >= end) break;
          const from = Math.max(base, token.from),
            to = Math.min(end, token.to);
          if (from < to)
            ranges.push({
              from: part.from + from - base,
              to: part.from + to - base,
              className: token.className,
            });
        }
        base = end;
      }
    },
  });
  return ranges;
}
