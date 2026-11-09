import { parser } from "@lezer/markdown";
import { pairedDeletion } from "./delimiters.ts";
import { selectWriting } from "./surface.ts";

export interface TextSelection {
  text: string;
  start: number;
  end: number;
}

function literalCode(text: string): boolean {
  const tree = parser.parse(text);
  for (
    let node: typeof tree.topNode | null = tree.resolveInner(text.length, -1);
    node;
    node = node.parent
  )
    if (node.name === "FencedCode" || node.name === "CodeBlock") return true;
  return false;
}

function replace(state: TextSelection, value: string, start: number, end = start): TextSelection {
  return {
    text: state.text.slice(0, state.start) + value + state.text.slice(state.end),
    start,
    end,
  };
}

export function wrapSelection(state: TextSelection, marker: string): TextSelection {
  const { text, start, end } = state;
  const selected = text.slice(start, end);
  if (
    text.slice(start - marker.length, start) === marker &&
    text.slice(end, end + marker.length) === marker
  )
    return {
      text: text.slice(0, start - marker.length) + selected + text.slice(end + marker.length),
      start: start - marker.length,
      end: end - marker.length,
    };
  const value = selected || "text";
  return replace(
    state,
    `${marker}${value}${marker}`,
    start + marker.length,
    start + marker.length + value.length,
  );
}

export function completeInput(
  state: TextSelection,
  token: string,
  extendPair = false,
): TextSelection | null {
  const { text, start, end } = state;
  if (start < 0 || end < start || end > text.length || token.length !== 1) return null;
  const selected = text.slice(start, end);
  const before = text.slice(0, start);
  const after = text.slice(end);
  if ("*_`".includes(token) && literalCode(before)) return null;
  if (selected && token === "*") return wrapSelection(state, "**");
  if (token === "[")
    return replace(state, `[${selected}]()`, start + (selected ? selected.length + 3 : 1));
  if (!selected && token === "]" && after.startsWith("]()"))
    return { ...state, start: start + 2, end: start + 2 };
  if (
    extendPair &&
    !selected &&
    "*_`".includes(token) &&
    before.endsWith(token) &&
    after.startsWith(token)
  )
    return replace(state, token + token, start + 1);
  if (!selected && ")]}*_`\"'".includes(token) && after.startsWith(token))
    return { ...state, start: start + 1, end: start + 1 };
  if (
    !selected &&
    token === " " &&
    /^\s*[-*+]$/.test(before.slice(before.lastIndexOf("\n") + 1)) &&
    after.startsWith(before.slice(-1))
  )
    return { text: `${before} ${after.slice(1)}`, start: start + 1, end: start + 1 };
  const pairs: Record<string, string> = {
    "(": ")",
    "{": "}",
    "*": "*",
    _: "_",
    "`": "`",
    '"': '"',
    "'": "'",
  };
  const close = pairs[token];
  if (!close) return null;
  if (selected) return replace(state, token + selected + close, start + 1, end + 1);
  if (
    ("*_\"'`".includes(token) && /(?:\p{L}|\p{N})$/u.test(before)) ||
    (after && !/^[\s)\]}.,;:!?]/.test(after))
  )
    return null;
  return replace(state, token + close, start + 1);
}

export function removePair(state: TextSelection): TextSelection | null {
  if (state.start !== state.end) return null;
  const before = state.text.charAt(state.start - 1);
  const after = state.text.charAt(state.start);
  if (before === "[" && state.text.slice(state.start, state.start + 3) === "]()")
    return {
      text: state.text.slice(0, state.start - 1) + state.text.slice(state.start + 3),
      start: state.start - 1,
      end: state.start - 1,
    };
  const pairs: Record<string, string> = {
    "(": ")",
    "[": "]",
    "{": "}",
    "*": "*",
    _: "_",
    "`": "`",
    '"': '"',
    "'": "'",
  };
  if (pairs[before] !== after) return null;
  return {
    text: state.text.slice(0, state.start - 1) + state.text.slice(state.start + 1),
    start: state.start - 1,
    end: state.start - 1,
  };
}

export function continueLine(state: TextSelection): TextSelection | null {
  if (state.start !== state.end) return null;
  const before = state.text.slice(0, state.start);
  const line = before.slice(before.lastIndexOf("\n") + 1);
  const lineStart = state.start - line.length;
  const content = line.trimStart();
  const indent = line.slice(0, line.length - content.length);
  const fence = /(?:`{3,}|~{3,})/.exec(content)?.[0];
  if (
    fence &&
    !/[`~]/.test(content.slice(fence.length)) &&
    state.text.slice(state.start).startsWith(fence)
  )
    return replace(state, `\n${indent}\n${indent}`, state.start + 1 + indent.length);
  if (literalCode(before))
    return indent ? replace(state, `\n${indent}`, state.start + indent.length + 1) : null;
  const containers = Array.from(
    content.matchAll(/(?:>[ \t]?|(?:[-+*]|\d{1,9}[.)])(?:[ \t]+|$))(?:\[[ xX]\][ \t])?/g),
  );
  let stop = 0;
  let last = 0;
  for (const container of containers) {
    if (container.index !== stop) break;
    last = stop;
    stop += container[0].length;
  }
  if (stop > 0) {
    if (!content.slice(stop).trim()) {
      const remaining = last ? indent + content.slice(0, last) : "";
      return {
        text: state.text.slice(0, lineStart) + remaining + state.text.slice(state.end),
        start: lineStart + remaining.length,
        end: lineStart + remaining.length,
      };
    }
    let marker = content.slice(last, stop);
    const size = marker.search(/[.)]/);
    if (size !== -1) {
      marker = `${Number(marker.slice(0, size)) + 1}${marker.slice(size)}`;
    }
    marker = marker.replace(/\[[ xX]\][ \t]/, "[ ] ");
    const prefix = indent + content.slice(0, last) + marker;
    return replace(state, `\n${prefix}`, state.start + prefix.length + 1);
  }
  return indent ? replace(state, `\n${indent}`, state.start + indent.length + 1) : null;
}

export function indentLines(state: TextSelection, size: number, remove: boolean): TextSelection {
  const before = state.text.slice(0, state.start);
  const start = before.lastIndexOf("\n") + 1;
  const end = state.end > state.start ? state.end - 1 : state.end;
  const lineEnd = state.text.indexOf("\n", end);
  const stop = lineEnd < 0 ? state.text.length : lineEnd;
  const lines = state.text.slice(start, stop).split("\n");
  const widths = lines.map((line) => {
    if (!remove) return size;
    const spaces = line.search(/[^ ]/);
    return -(line.startsWith("\t") ? 1 : Math.min(size, spaces < 0 ? line.length : spaces));
  });
  const value = lines
    .map((line, index) => (remove ? line.slice(-(widths[index] ?? 0)) : " ".repeat(size) + line))
    .join("\n");
  return {
    text: state.text.slice(0, start) + value + state.text.slice(stop),
    start: Math.max(start, state.start + (widths[0] ?? 0)),
    end: Math.max(start, state.end + widths.reduce((sum, width) => sum + width, 0)),
  };
}

export function tabTarget(state: TextSelection, backward = false): TextSelection | null {
  let target: number | null = null;
  const tree = parser.parse(state.text);
  for (
    let node: typeof tree.topNode | null = tree.resolveInner(state.start, backward ? -1 : 1);
    node && target === null;
    node = node.parent
  ) {
    if (state.end > node.to) continue;
    if (node.name === "Link" || node.name === "Image") {
      const marks = node.getChildren("LinkMark");
      const destination = marks.find((mark) => state.text.slice(mark.from, mark.to) === "(");
      const label = marks[0];
      if (destination && label) {
        if (backward && state.start >= destination.to) target = label.to;
        else if (!backward && state.end < destination.to) target = destination.to;
        else if (!backward && state.end < node.to) target = node.to;
      }
    } else if (["Emphasis", "StrongEmphasis", "InlineCode"].includes(node.name)) {
      const open = node.firstChild;
      const close = node.lastChild;
      if (open && close && state.start >= open.to && state.end <= close.from)
        target = backward ? open.to : node.to;
    }
  }
  if (target === null && !backward && state.start === state.end) {
    const close = state.text.charAt(state.start);
    const pairs: Record<string, string> = { ")": "(", "]": "[", "}": "{", '"': '"', "'": "'" };
    const open = pairs[close];
    if (open && state.text.slice(0, state.start).includes(open)) target = state.start + 1;
    const marker = /^[*_`]+/.exec(state.text.slice(state.start))?.[0];
    if (marker && state.text.slice(0, state.start).endsWith(marker))
      target = state.start + marker.length;
  }
  return target === null ? null : { ...state, start: target, end: target };
}

export function initInput(
  body: HTMLTextAreaElement,
  writable: () => boolean,
  enabled: () => boolean,
  tabSize: () => number,
): void {
  let escapeTab = false;
  let emptyPair: TextSelection | null = null;
  const state = (): TextSelection => ({
    text: body.value,
    start: body.selectionStart,
    end: body.selectionEnd,
  });
  function apply(edit: TextSelection): void {
    const changed = edit.text !== body.value;
    body.value = edit.text;
    body.setSelectionRange(edit.start, edit.end);
    if (changed) body.dispatchEvent(new Event("input", { bubbles: true }));
    else selectWriting(body, edit.start, edit.end);
  }
  body.addEventListener("beforeinput", (event) => {
    const selection = state();
    const extend =
      emptyPair?.text === selection.text &&
      emptyPair.start === selection.start &&
      emptyPair.end === selection.end;
    emptyPair = null;
    if (
      !enabled() ||
      !writable() ||
      event.isComposing ||
      !event.cancelable ||
      (event.inputType !== "insertText" && !event.inputType.startsWith("delete"))
    )
      return;
    if (event.inputType.startsWith("delete")) {
      const direction = event.inputType === "deleteContentForward" ? "forward" : "backward";
      const edit =
        selection.start !== selection.end ||
        /^deleteContent(?:Backward|Forward)$/.test(event.inputType)
          ? pairedDeletion(selection, direction)
          : null;
      if (edit) {
        event.preventDefault();
        apply(edit);
      }
      return;
    }
    if (!event.data) return;
    const edit = completeInput(selection, event.data, extend);
    if (edit) {
      event.preventDefault();
      if (edit.text !== selection.text) emptyPair = edit;
      apply(edit);
    }
  });
  body.addEventListener("keydown", (event) => {
    if (
      !enabled() ||
      !writable() ||
      event.isComposing ||
      event.key === "Shift" ||
      event.ctrlKey ||
      event.metaKey ||
      event.altKey
    )
      return;
    if (event.key === "Escape") {
      escapeTab = true;
      return;
    }
    if (event.key === "Tab" && escapeTab) {
      escapeTab = false;
      return;
    }
    escapeTab = false;
    const edit =
      event.key === "Backspace"
        ? (removePair(state()) ?? pairedDeletion(state(), "backward"))
        : event.key === "Delete"
          ? pairedDeletion(state(), "forward")
          : event.key === "Enter"
            ? continueLine(state())
            : event.key === "Tab"
              ? (tabTarget(state(), event.shiftKey) ??
                indentLines(state(), tabSize(), event.shiftKey))
              : null;
    if (edit) {
      event.preventDefault();
      apply(edit);
    }
  });
}
