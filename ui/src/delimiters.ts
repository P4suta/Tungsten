import { parser, Strikethrough } from "@lezer/markdown";
import type { TextSelection } from "./input.ts";

const markdown = parser.configure(Strikethrough);
interface Range {
  from: number;
  to: number;
}

function deleteRanges(state: TextSelection, ranges: Range[], caret: number): TextSelection {
  const sorted = ranges.sort((a, b) => a.from - b.from);
  let shift = 0;
  let text = state.text;
  for (const range of sorted) shift += Math.max(0, Math.min(caret, range.to) - range.from);
  for (const range of sorted.reverse()) text = text.slice(0, range.from) + text.slice(range.to);
  return { text, start: caret - shift, end: caret - shift };
}

export function pairedDeletion(
  state: TextSelection,
  direction: "backward" | "forward",
): TextSelection | null {
  if (state.start < 0 || state.end < state.start || state.end > state.text.length) return null;
  const from =
    state.start === state.end && direction === "backward" ? state.start - 1 : state.start;
  const to = state.start === state.end && direction === "forward" ? state.end + 1 : state.end;
  if (from < 0 || to > state.text.length || from === to) return null;
  let result: TextSelection | null = null;
  markdown.parse(state.text).iterate({
    enter(ref) {
      if (result || from < ref.from || to > ref.to) return;
      if (ref.name === "FencedCode") {
        const marks = ref.node.getChildren("CodeMark");
        const open = marks[0],
          close = marks[1];
        if (!open || !close) return;
        const lineStart = (at: number): number => state.text.lastIndexOf("\n", at - 1) + 1;
        const lineEnd = (at: number): number => {
          const end = state.text.indexOf("\n", at);
          return end < 0 ? state.text.length : end;
        };
        const opening = from >= open.from && from < open.to && to <= lineEnd(open.to) + 1;
        const closing = from >= close.from && from < close.to && to <= lineEnd(close.to) + 1;
        if (!opening && !closing) return;
        const ranges = [open, close].map((mark) => {
          const start = lineStart(mark.from),
            end = lineEnd(mark.to);
          return {
            from: state.text.slice(start, mark.from).trim() ? mark.from : start,
            to: state.text.slice(start, mark.from).trim()
              ? end
              : Math.min(state.text.length, end + 1),
          };
        });
        result = deleteRanges(state, ranges, from);
        return;
      }
      let pairs: [Range, Range][] = [];
      if (["Emphasis", "StrongEmphasis", "Strikethrough", "InlineCode"].includes(ref.name)) {
        const open = ref.node.firstChild,
          close = ref.node.lastChild;
        if (open && close && open !== close) pairs = [[open, close]];
      } else if (ref.name === "Link" || ref.name === "Image") {
        const marks = ref.node.getChildren("LinkMark");
        for (const [left, right] of [
          ["[", "]"],
          ["(", ")"],
        ]) {
          const open = marks.find((mark) =>
            state.text.slice(mark.from, mark.to).endsWith(left ?? ""),
          );
          const close = marks.find((mark) => state.text.slice(mark.from, mark.to) === right);
          if (open && close) pairs.push([{ from: open.to - 1, to: open.to }, close]);
        }
      }
      for (const [open, close] of pairs) {
        const opening = from >= open.from && to <= open.to;
        const closing = from >= close.from && to <= close.to;
        if (!opening && !closing) continue;
        const opposite = opening ? close : open;
        const count = to - from;
        if (count > opposite.to - opposite.from) continue;
        result = deleteRanges(
          state,
          [
            { from, to },
            { from: opposite.to - count, to: opposite.to },
          ],
          from,
        );
      }
    },
  });
  return result;
}
