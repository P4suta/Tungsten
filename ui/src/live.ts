import {
  defaultKeymap,
  deleteCharBackward,
  deleteCharForward,
  history,
  historyKeymap,
  insertNewlineAndIndent,
  isolateHistory,
} from "@codemirror/commands";
import {
  Compartment,
  EditorState,
  type Extension,
  StateEffect,
  StateField,
} from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, keymap, WidgetType } from "@codemirror/view";
import { parser } from "@lezer/markdown";
import { needsWidget, type TextRange, textLayout } from "./live-ranges.js";
import { createMediaPreviews } from "./media.js";
import { previewCharacters, previewLines } from "./rendering.js";
import { registerSurface } from "./surface.js";
import type { CodeHighlight } from "./syntax.js";

export interface RenderedBlock {
  from: number;
  to: number;
  html: string;
}

interface LivePort {
  api(path: string): Promise<unknown>;
  help(): { spellcheck: boolean; tabSize: number };
  attach(files: File[]): void;
}

export interface LiveEditor {
  sync(identity: string): void;
  show(): void;
  render(body: string, blocks: RenderedBlock[]): void;
  anchor(index: number): void;
}

class MarkdownView extends EditorView {
  static readonly EDIT_CONTEXT = false;
}

class Prefix extends WidgetType {
  constructor(
    readonly text: string,
    readonly width: number,
  ) {
    super();
  }
  eq(other: Prefix): boolean {
    return other.text === this.text && other.width === this.width;
  }
  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = this.text ? "live-prefix live-list-marker" : "live-prefix";
    span.textContent = this.text;
    span.style.width = `${this.width}ch`;
    return span;
  }
}

const renderedHTML = new WeakMap<HTMLElement, string>();

function makeLinksFocusable(root: HTMLElement): void {
  for (const link of root.querySelectorAll<HTMLAnchorElement>("a[href]")) {
    if (link.closest(".live-link")) continue;
    if (/^https?:$/.test(link.protocol) && link.origin !== location.origin) {
      link.target = "_blank";
      link.rel = "noopener noreferrer";
    }
    const target = document.createElement("span");
    target.className = "live-link";
    target.setAttribute("role", "link");
    target.tabIndex = 0;
    target.setAttribute(
      "aria-label",
      link.getAttribute("aria-label") ||
        link.textContent?.trim() ||
        link.querySelector("img")?.alt ||
        link.href,
    );
    link.tabIndex = -1;
    link.setAttribute("aria-hidden", "true");
    link.before(target);
    target.append(link);
    target.addEventListener("click", (event) => {
      if (event.target !== target || event.defaultPrevented) return;
      link.dispatchEvent(
        new MouseEvent("click", {
          bubbles: true,
          cancelable: true,
          ctrlKey: event.ctrlKey,
          metaKey: event.metaKey,
          shiftKey: event.shiftKey,
          altKey: event.altKey,
        }),
      );
    });
    target.addEventListener("keydown", (event) => {
      if (
        event.target !== target ||
        event.key !== "Enter" ||
        event.isComposing ||
        event.ctrlKey ||
        event.metaKey ||
        event.altKey ||
        event.shiftKey
      )
        return;
      event.preventDefault();
      event.stopPropagation();
      link.click();
    });
  }
}

class RenderedWidget extends WidgetType {
  constructor(
    readonly block: RenderedBlock,
    readonly mounted: () => void,
  ) {
    super();
  }
  eq(other: RenderedWidget): boolean {
    return other.block.from === this.block.from && other.block.html === this.block.html;
  }
  toDOM(view: EditorView): HTMLElement {
    const element = document.createElement("div");
    element.className = "live-block";
    element.innerHTML = this.block.html;
    element.dataset.from = String(this.block.from);
    renderedHTML.set(element, this.block.html);
    makeLinksFocusable(element);
    element.addEventListener("pointerdown", (event) => {
      if (
        !(event.target instanceof Element) ||
        event.target.closest(
          "a, .live-link, button, input, select, textarea, label, summary, audio, video",
        )
      )
        return;
      event.preventDefault();
      view.focus();
      view.dispatch({
        selection: { anchor: Math.min(Number(element.dataset.from), view.state.doc.length) },
      });
    });
    this.mounted();
    return element;
  }
  updateDOM(element: HTMLElement): boolean {
    if (renderedHTML.get(element) !== this.block.html) return false;
    element.dataset.from = String(this.block.from);
    return true;
  }
  ignoreEvent(): boolean {
    return true;
  }
}

export function initLive(body: HTMLTextAreaElement, host: HTMLElement, port: LivePort): LiveEditor {
  const label = body.labels?.[0];
  const media = createMediaPreviews(port.api);
  let mounted = false;
  const refreshMedia = (): void => {
    if (mounted) return;
    mounted = true;
    requestAnimationFrame(() => {
      mounted = false;
      media.update(host, view.state.doc.toString());
      view.requestMeasure();
    });
  };
  const rendered = StateEffect.define<RenderedBlock[]>();
  const highlighted = StateEffect.define<CodeHighlight[]>();
  const syntax = StateField.define<CodeHighlight[]>({
    create: () => [],
    update(value, transaction) {
      for (const effect of transaction.effects) if (effect.is(highlighted)) return effect.value;
      return transaction.docChanged ? [] : value;
    },
  });
  let highlightGeneration = 0;
  let highlightTimer: number | undefined;
  function highlightCode(text: string): void {
    const generation = ++highlightGeneration;
    window.clearTimeout(highlightTimer);
    if (text.length > 64 * 1024 || !/^(?: {0,3}|> ?)(?:`{3,}|~{3,})/m.test(text)) return;
    highlightTimer = window.setTimeout(() => {
      void import("./syntax.js").then(async ({ codeHighlights }) => {
        const ranges = await codeHighlights(text);
        if (generation !== highlightGeneration || text !== view.state.doc.toString()) return;
        view.dispatch({ effects: highlighted.of(ranges) });
      });
    }, 120);
  }
  const blocks = StateField.define<RenderedBlock[]>({
    create: () => [],
    update(value, transaction) {
      for (const effect of transaction.effects) if (effect.is(rendered)) return effect.value;
      if (!transaction.docChanged) return value;
      return value.flatMap((block) => {
        const from = transaction.changes.mapPos(block.from, 1);
        const to = transaction.changes.mapPos(block.to, -1);
        return from < to &&
          transaction.startState.doc.sliceString(block.from, block.to) ===
            transaction.state.doc.sliceString(from, to)
          ? [{ ...block, from, to }]
          : [];
      });
    },
  });
  function decorations(state: EditorState): DecorationSet {
    if (state.doc.length > previewCharacters || state.doc.lines > previewLines)
      return Decoration.none;
    const ranges = [];
    for (const token of state.field(syntax))
      ranges.push(Decoration.mark({ class: token.className }).range(token.from, token.to));
    const replaced: TextRange[] = [];
    const selection = state.selection.main;
    const from = state.doc.lineAt(selection.from).from;
    const to = state.doc.lineAt(selection.to).to;
    for (const block of state.field(blocks)) {
      if (!needsWidget(block.html)) continue;
      const end =
        block.to > block.from && state.doc.sliceString(block.to - 1, block.to) === "\n"
          ? block.to - 1
          : block.to;
      if (
        block.from > state.doc.length ||
        end > state.doc.length ||
        end <= block.from ||
        (from < end && to >= block.from)
      )
        continue;
      ranges.push(
        Decoration.replace({ block: true, widget: new RenderedWidget(block, refreshMedia) }).range(
          block.from,
          end,
        ),
      );
      replaced.push({ from: block.from, to: end });
    }
    const text = state.doc.toString();
    const layout = textLayout(text, selection);
    const covered = (range: TextRange): boolean =>
      replaced.some((block) => range.from < block.to && range.to > block.from);
    for (const mark of layout.marks)
      if (!covered(mark))
        ranges.push(
          Decoration.mark({ tagName: mark.tag, class: `live-${mark.tag}` }).range(
            mark.from,
            mark.to,
          ),
        );
    for (const hidden of layout.hidden)
      if (!covered(hidden)) ranges.push(Decoration.replace({}).range(hidden.from, hidden.to));
    for (const prefix of layout.prefixes)
      if (!covered(prefix))
        ranges.push(
          (prefix.text === null
            ? Decoration.mark({ class: "live-prefix" })
            : Decoration.replace({ widget: new Prefix(prefix.text, prefix.to - prefix.from) })
          ).range(prefix.from, prefix.to),
        );
    for (const line of layout.lines)
      if (!covered(state.doc.lineAt(line.from))) {
        const className = {
          list: "live-list-line",
          quote: "live-quote",
          code: "live-code-block",
          fence: "live-code-fence",
        }[line.kind];
        ranges.push(Decoration.line({ class: className }).range(line.from));
      }
    for (const heading of layout.headings)
      if (!covered(state.doc.lineAt(heading.from)))
        ranges.push(
          Decoration.line({
            class: `live-heading live-heading-${heading.level}`,
            attributes: { role: "heading", "aria-level": String(heading.level) },
          }).range(heading.from),
        );
    if (state.doc.line(1).length && !covered({ from: 0, to: state.doc.line(1).to + 1 })) {
      ranges.push(Decoration.line({ class: "live-title" }).range(0));
    }
    return Decoration.set(ranges, true);
  }
  const painted = StateField.define<DecorationSet>({
    create: decorations,
    update: (_, transaction) => decorations(transaction.state),
    provide: (field) => EditorView.decorations.from(field),
  });
  const access = new Compartment();
  let identity = "";
  let syncing = false;
  function select(): void {
    const range = view.state.selection.main;
    body.setSelectionRange(
      Math.min(range.from, body.value.length),
      Math.min(range.to, body.value.length),
    );
  }
  function forward(event: KeyboardEvent): boolean {
    select();
    const copy = new KeyboardEvent(event.type, {
      key: event.key,
      code: event.code,
      ctrlKey: event.ctrlKey,
      metaKey: event.metaKey,
      shiftKey: event.shiftKey,
      altKey: event.altKey,
      isComposing: event.isComposing,
      bubbles: true,
      cancelable: true,
    });
    body.dispatchEvent(copy);
    if (copy.defaultPrevented) event.stopPropagation();
    return copy.defaultPrevented;
  }
  function configuration(): Extension[] {
    return [
      EditorState.readOnly.of(body.disabled || body.readOnly),
      EditorView.editable.of(!body.disabled && !body.readOnly),
      EditorState.tabSize.of(port.help().tabSize),
      EditorView.contentAttributes.of({
        spellcheck: String(port.help().spellcheck),
        "aria-readonly": String(body.disabled || body.readOnly),
      }),
    ];
  }
  const extensions = [
    syntax,
    blocks,
    painted,
    access.of(configuration()),
    EditorView.cspNonce.of(
      document.querySelector<HTMLMetaElement>('meta[name="style-nonce"]')?.content ?? "",
    ),
    EditorView.lineWrapping,
    EditorView.contentAttributes.of({
      role: "textbox",
      tabindex: "0",
      "aria-label": "Note",
      "aria-multiline": "true",
      "aria-describedby": "writing-help",
    }),
    EditorView.domEventHandlers({
      keydown(event) {
        if (forward(event)) return true;
        if (
          (event.key === "ArrowUp" || event.key === "ArrowDown") &&
          !event.ctrlKey &&
          !event.metaKey &&
          !event.altKey &&
          !event.isComposing &&
          !view.composing
        ) {
          const selection = view.state.selection.main;
          const line = view.state.doc.lineAt(selection.head);
          const number = line.number + (event.key === "ArrowUp" ? -1 : 1);
          if (number >= 1 && number <= view.state.doc.lines) {
            const next = view.state.doc.line(number);
            const hidden = view.state
              .field(blocks)
              .some(
                (block) =>
                  next.from >= block.from &&
                  next.from < block.to &&
                  !(selection.head >= block.from && selection.head < block.to),
              );
            const caret = view.coordsAtPos(selection.head);
            const edge = view.coordsAtPos(event.key === "ArrowUp" ? line.from : line.to);
            if (
              hidden &&
              caret &&
              edge &&
              Math.abs(caret.top - edge.top) < (caret.bottom - caret.top) / 2
            ) {
              let head = Math.min(next.to, next.from + selection.head - line.from);
              if (/[\uDC00-\uDFFF]/.test(view.state.doc.sliceString(head, head + 1))) head--;
              view.dispatch({
                selection: { anchor: event.shiftKey ? selection.anchor : head, head },
                scrollIntoView: true,
              });
              return true;
            }
          }
        }
        return false;
      },
      keyup: (event) => {
        forward(event);
      },
      beforeinput(event) {
        if (view.composing || event.isComposing) return false;
        select();
        const copy = new InputEvent("beforeinput", {
          data: event.data,
          inputType: event.inputType,
          isComposing: event.isComposing,
          cancelable: true,
        });
        body.dispatchEvent(copy);
        return copy.defaultPrevented;
      },
      blur(event) {
        if (event.relatedTarget instanceof Node && host.contains(event.relatedTarget)) return;
        body.dispatchEvent(new Event("blur"));
      },
      copy(event) {
        if (!event.clipboardData || view.state.selection.ranges.every((range) => range.empty))
          return false;
        event.clipboardData.setData(
          "text/plain",
          view.state.selection.ranges
            .map((range) => view.state.sliceDoc(range.from, range.to))
            .join("\n"),
        );
        return true;
      },
      cut(event) {
        if (!event.clipboardData || view.state.selection.ranges.every((range) => range.empty))
          return false;
        event.clipboardData.setData(
          "text/plain",
          view.state.selection.ranges
            .map((range) => view.state.sliceDoc(range.from, range.to))
            .join("\n"),
        );
        if (!view.state.readOnly)
          view.dispatch(view.state.replaceSelection(""), {
            userEvent: "delete.cut",
            annotations: isolateHistory.of("full"),
          });
        return true;
      },
      paste(event) {
        if (event.clipboardData?.files.length) {
          select();
          port.attach(Array.from(event.clipboardData.files));
          return true;
        }
        const text = event.clipboardData?.getData("text/plain");
        if (!text || view.state.readOnly) return false;
        view.dispatch(view.state.replaceSelection(text), {
          userEvent: "input.paste",
          annotations: isolateHistory.of("full"),
        });
        return true;
      },
      drop(event) {
        if (!event.dataTransfer?.files.length) return false;
        select();
        port.attach(Array.from(event.dataTransfer.files));
        return true;
      },
    }),
    history(),
    EditorState.transactionExtender.of((transaction) =>
      ["move.line", "input.copyline", "delete.line"].some((event) => transaction.isUserEvent(event))
        ? { annotations: isolateHistory.of("full") }
        : null,
    ),
    keymap.of([...historyKeymap, ...defaultKeymap]),
    EditorView.updateListener.of((update) => {
      if (syncing) return;
      select();
      if (update.docChanged) {
        body.value = update.state.doc.toString();
        highlightCode(body.value);
        select();
        body.dispatchEvent(new Event("input", { bubbles: true }));
      }
      if (update.selectionSet || update.docChanged) refreshMedia();
    }),
    EditorView.theme({
      "&": { color: "#000", backgroundColor: "#fff", fontSize: "inherit", minHeight: "1lh" },
      "&.cm-focused": { outline: "none" },
      ".cm-scroller": {
        fontFamily: "inherit",
        lineHeight: "var(--line-height, 1.75)",
        overflow: "visible",
      },
      ".cm-content": { padding: "0", caretColor: "#000" },
      ".cm-line": { padding: "0", minHeight: "1lh" },
      ".cm-cursor, .cm-dropCursor": { borderLeftColor: "#000" },
      ".cm-selectionBackground, &.cm-focused .cm-selectionBackground, .cm-selectionLayer .cm-selectionBackground":
        { backgroundColor: "#000" },
      ".cm-content ::selection, .cm-line::selection, .cm-line *::selection": {
        backgroundColor: "#000",
        color: "#fff",
      },
      ".cm-gutters": { color: "#000", backgroundColor: "#fff", borderRightColor: "#000" },
      ".cm-announced": { position: "fixed" },
    }),
  ];
  const make = (text: string): EditorState => EditorState.create({ doc: text, extensions });
  const view = new MarkdownView({ state: make(body.value), parent: host });
  highlightCode(body.value);
  new MutationObserver(() => makeLinksFocusable(host)).observe(host, {
    childList: true,
    subtree: true,
  });
  view.contentDOM.addEventListener(
    "keydown",
    (event) => {
      if (
        !["Enter", "Backspace", "Delete"].includes(event.key) ||
        event.target !== view.contentDOM ||
        event.defaultPrevented ||
        event.isComposing ||
        view.composing ||
        event.ctrlKey ||
        event.metaKey ||
        event.altKey ||
        body.disabled ||
        body.readOnly
      )
        return;
      if (event.key !== "Enter") {
        const handled =
          forward(event) ||
          (!view.state.selection.main.empty &&
            (event.key === "Backspace" ? deleteCharBackward(view) : deleteCharForward(view)));
        if (handled) {
          event.preventDefault();
          event.stopImmediatePropagation();
        }
        return;
      }
      event.preventDefault();
      event.stopImmediatePropagation();
      if (!forward(event)) insertNewlineAndIndent(view);
    },
    { capture: true },
  );
  host.inert = body.disabled;
  registerSurface({
    element: view.contentDOM,
    visible: () => !host.hidden && host.checkVisibility(),
    afterLayout: (callback) => {
      view.requestMeasure({
        key: callback,
        read: () => null,
        write: () => requestAnimationFrame(callback),
      });
    },
    focus: () => {
      const selection = view.state.selection;
      view.focus();
      view.dispatch({ selection });
    },
    select: (from, to) => {
      if (!syncing && !view.composing) view.dispatch({ selection: { anchor: from, head: to } });
    },
    caret: () => {
      const selection = document.getSelection();
      if (selection?.focusNode && view.contentDOM.contains(selection.focusNode)) {
        const range = document.createRange();
        range.setStart(selection.focusNode, selection.focusOffset);
        range.collapse(true);
        const rect = range.getBoundingClientRect();
        if (rect.height > 0) return rect;
      }
      const box = view.coordsAtPos(view.state.selection.main.head);
      return box ? new DOMRect(box.left, box.top, 1, box.bottom - box.top) : null;
    },
  });
  const sync = (nextIdentity: string): void => {
    if (syncing) return;
    syncing = true;
    try {
      host.inert = body.disabled && identity === "";
      if (identity !== nextIdentity) {
        identity = nextIdentity;
        media.clear();
        view.setState(make(body.value));
        highlightCode(body.value);
      } else if (body.value !== view.state.doc.toString()) {
        let start = 0;
        const previous = view.state.doc.toString();
        while (
          start < previous.length &&
          start < body.value.length &&
          previous[start] === body.value[start]
        )
          start++;
        let end = previous.length;
        let targetEnd = body.value.length;
        while (
          end > start &&
          targetEnd > start &&
          previous[end - 1] === body.value[targetEnd - 1]
        ) {
          end--;
          targetEnd--;
        }
        view.dispatch({
          changes: { from: start, to: end, insert: body.value.slice(start, targetEnd) },
          annotations: isolateHistory.of("full"),
          selection: { anchor: body.selectionStart, head: body.selectionEnd },
        });
        highlightCode(body.value);
      }
      const readOnly = body.disabled || body.readOnly;
      if (
        view.state.readOnly !== readOnly ||
        view.state.tabSize !== port.help().tabSize ||
        view.contentDOM.spellcheck !== port.help().spellcheck
      )
        view.dispatch({ effects: access.reconfigure(configuration()) });
    } finally {
      syncing = false;
    }
  };
  body.addEventListener("input", () => sync(identity));
  return {
    sync,
    show() {
      host.hidden = false;
      body.hidden = true;
      if (label) label.htmlFor = "";
      view.contentDOM.setAttribute("aria-label", "Note");
      if (!view.composing)
        view.dispatch({ selection: { anchor: body.selectionStart, head: body.selectionEnd } });
      view.requestMeasure();
    },
    anchor(index) {
      const positions: number[] = [];
      parser.parse(view.state.doc.toString()).iterate({
        enter(ref) {
          if (/^(?:ATX|Setext)Heading[1-6]$/.test(ref.name)) positions.push(ref.from);
        },
      });
      const position = positions[index];
      if (position === undefined) return;
      view.dispatch({
        selection: { anchor: position },
        effects: EditorView.scrollIntoView(position, { y: "start" }),
      });
      view.focus();
    },
    render(text, value) {
      if (text !== body.value || view.composing) return;
      view.dispatch({ effects: rendered.of(value) });
      refreshMedia();
    },
  };
}
