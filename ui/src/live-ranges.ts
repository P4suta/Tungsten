import { parser, Strikethrough } from "@lezer/markdown";

const markdown = parser.configure(Strikethrough);

export interface TextRange {
  from: number;
  to: number;
}

export interface TextMark extends TextRange {
  tag: "strong" | "em" | "s" | "code";
}

export interface TextLayout {
  marks: TextMark[];
  hidden: TextRange[];
  headings: { from: number; level: number }[];
  prefixes: (TextRange & { text: string | null })[];
  lines: { from: number; kind: "list" | "quote" | "code" | "fence" }[];
}

export function textLayout(text: string, selection: TextRange): TextLayout {
  const layout: TextLayout = { marks: [], hidden: [], headings: [], prefixes: [], lines: [] };
  const lineStart = (at: number): number => text.slice(0, at).lastIndexOf("\n") + 1;
  function lines(from: number, to: number, kind: TextLayout["lines"][number]["kind"]): void {
    const first = lineStart(from);
    for (const match of text.slice(first, to).matchAll(/^/gm)) {
      const start = first + match.index;
      if (!layout.lines.some((line) => line.from === start && line.kind === kind))
        layout.lines.push({ from: start, kind });
    }
  }
  markdown.parse(text).iterate({
    enter(ref) {
      if (ref.name === "FencedCode" || ref.name === "CodeBlock") lines(ref.from, ref.to, "code");
      if (ref.name === "Blockquote") lines(ref.from, ref.to, "quote");
      const prefix =
        ref.name === "ListMark"
          ? /[-+*]/.test(text.slice(ref.from, ref.to))
            ? "•"
            : text.slice(ref.from, ref.to).replace(")", ".")
          : ref.name === "QuoteMark" ||
              (ref.name === "CodeMark" && ref.node.parent?.name === "FencedCode")
            ? ""
            : null;
      if (prefix !== null) {
        layout.prefixes.push({
          from: ref.from,
          to: ref.to,
          text: selection.from <= ref.to && selection.to >= ref.from ? null : prefix,
        });
        if (ref.name === "ListMark") lines(ref.from, ref.to, "list");
        if (ref.name === "CodeMark") lines(ref.from, ref.to, "fence");
      }
      const level =
        [
          "ATXHeading1",
          "ATXHeading2",
          "ATXHeading3",
          "ATXHeading4",
          "ATXHeading5",
          "ATXHeading6",
        ].indexOf(ref.name) + 1;
      if (level > 0)
        layout.headings.push({
          from: lineStart(ref.from),
          level,
        });
      const tag =
        ref.name === "StrongEmphasis"
          ? "strong"
          : ref.name === "Emphasis"
            ? "em"
            : ref.name === "Strikethrough"
              ? "s"
              : ref.name === "InlineCode"
                ? "code"
                : null;
      if (!tag) return;
      const open = ref.node.firstChild;
      const close = ref.node.lastChild;
      if (!open || !close) return;
      layout.marks.push({ from: open.to, to: close.from, tag });
      if (selection.from > ref.to || selection.to < ref.from)
        layout.hidden.push({ from: open.from, to: open.to }, { from: close.from, to: close.to });
    },
  });
  return layout;
}

export function needsWidget(html: string): boolean {
  return /<(?:a|img|input|table|hr)\b/.test(html);
}
