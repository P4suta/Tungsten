import { writingElement } from "./surface.js";
import { caretBox } from "./writing.js";

export interface SlashRange {
  start: number;
  end: number;
  query: string;
}

export type SlashCommand =
  | { id: string; label: string; kind: "block"; text: string; cursor: number }
  | { id: string; label: string; kind: "file"; accept: string };

export const commands: readonly SlashCommand[] = [
  { id: "file", label: "File", kind: "file", accept: "" },
  { id: "heading", label: "Heading 1", kind: "block", text: "# ", cursor: 2 },
  { id: "heading2", label: "Heading 2", kind: "block", text: "## ", cursor: 3 },
  { id: "heading3", label: "Heading 3", kind: "block", text: "### ", cursor: 4 },
  { id: "bullet", label: "Bullet list", kind: "block", text: "- ", cursor: 2 },
  { id: "number", label: "Numbered list", kind: "block", text: "1. ", cursor: 3 },
  { id: "task", label: "Task list", kind: "block", text: "- [ ] ", cursor: 6 },
  { id: "quote", label: "Quote", kind: "block", text: "> ", cursor: 2 },
  { id: "code", label: "Code block", kind: "block", text: "```\n\n```", cursor: 4 },
  { id: "link", label: "Link", kind: "block", text: "[]()", cursor: 1 },
  {
    id: "table",
    label: "Table",
    kind: "block",
    text: "| Column | Column |\n| --- | --- |\n|  |  |",
    cursor: 2,
  },
  { id: "divider", label: "Divider", kind: "block", text: "---\n\n", cursor: 5 },
];

export function slashRange(text: string, start: number, end: number): SlashRange | null {
  if (start !== end || start < 0 || end > text.length) return null;
  const line = start === 0 ? 0 : text.lastIndexOf("\n", start - 1) + 1;
  const query = /^( {0,3})\/([a-z0-9]*)$/i.exec(text.slice(line, start));
  if (!query) return null;
  let fence: { marker: string; size: number } | null = null;
  for (const previous of text.slice(0, line).split("\n")) {
    const match = /^ {0,3}(`{3,}|~{3,})(.*)$/.exec(previous);
    if (!match?.[1]) continue;
    if (!fence) fence = { marker: match[1][0] ?? "", size: match[1].length };
    else if (match[1][0] === fence.marker && match[1].length >= fence.size && !match[2]?.trim())
      fence = null;
  }
  if (fence) return null;
  return {
    start: line + (query[1]?.length ?? 0),
    end,
    query: (query[2] ?? "").toLocaleLowerCase(),
  };
}

export function matchingCommands(query: string): readonly SlashCommand[] {
  return commands.filter((command) =>
    `${command.id} ${command.label}`.toLocaleLowerCase().includes(query),
  );
}

export function initInsertions(
  body: HTMLTextAreaElement,
  composing: () => boolean,
  execute: (command: SlashCommand, range: SlashRange) => void,
  suggest: (
    text: string,
    start: number,
    end: number,
  ) => { range: SlashRange; commands: readonly SlashCommand[] } | null,
): () => void {
  const target = document.getElementById("slash-menu");
  if (!target) throw new Error("Missing slash menu");
  const menu = target;
  let range: SlashRange | null = null;
  let options: readonly SlashCommand[] = [];
  let selected = 0;
  let dismissed = "";

  function hide(): void {
    menu.hidden = true;
    writingElement(body).removeAttribute("aria-activedescendant");
    writingElement(body).removeAttribute("aria-controls");
    writingElement(body).removeAttribute("aria-autocomplete");
    range = null;
  }
  function highlight(): void {
    for (const [index, option] of Array.from(menu.children).entries())
      option.setAttribute("aria-selected", String(index === selected));
    const active = menu.children[selected];
    if (active) {
      writingElement(body).setAttribute("aria-activedescendant", active.id);
      active.scrollIntoView({ block: "nearest" });
    }
  }
  function choose(): void {
    const command = options[selected];
    const target = range;
    if (!command || !target || composing()) return;
    hide();
    execute(command, target);
  }
  function update(): void {
    if (
      composing() ||
      !writingElement(body).contains(document.activeElement) ||
      body.disabled ||
      body.readOnly ||
      !writingElement(body).checkVisibility()
    ) {
      hide();
      return;
    }
    const links = suggest(body.value, body.selectionStart, body.selectionEnd);
    const target = links?.range ?? slashRange(body.value, body.selectionStart, body.selectionEnd);
    if (!target || dismissed === `${body.value}\0${body.selectionStart}`) {
      hide();
      return;
    }
    const previous = range?.query;
    range = target;
    options = links?.commands ?? matchingCommands(target.query);
    if (!options.length) {
      hide();
      return;
    }
    if (previous !== target.query) selected = 0;
    selected = Math.min(selected, options.length - 1);
    menu.setAttribute("aria-label", links ? "Link to note" : "Insert into note");
    menu.replaceChildren();
    for (const [index, command] of options.entries()) {
      const option = document.createElement("div");
      option.id = links ? `note-option-${index}` : `slash-${command.id}`;
      option.setAttribute("role", "option");
      option.setAttribute("aria-selected", String(index === selected));
      const label = document.createElement("span");
      label.textContent = command.label;
      const shortcut = document.createElement("code");
      shortcut.textContent = `/${command.id}`;
      option.append(label);
      if (!links) option.append(shortcut);
      option.addEventListener("pointermove", () => {
        selected = index;
        highlight();
      });
      option.addEventListener("click", () => {
        selected = index;
        choose();
      });
      menu.append(option);
    }
    menu.hidden = false;
    writingElement(body).setAttribute("aria-controls", menu.id);
    writingElement(body).setAttribute("aria-autocomplete", "list");
    const caret = caretBox(body);
    const bounds = menu.getBoundingClientRect();
    const viewport = visualViewport;
    const top = viewport?.offsetTop ?? 0;
    const height = viewport?.height ?? innerHeight;
    const below = caret.top + caret.height + 4;
    menu.style.top = `${Math.max(top + 8, below + bounds.height < top + height - 8 ? below : caret.top - bounds.height - 4)}px`;
    menu.style.left = `${Math.max(8, Math.min(caret.left, innerWidth - bounds.width - 8))}px`;
    highlight();
  }
  body.addEventListener("input", () => {
    dismissed = "";
    requestAnimationFrame(update);
  });
  body.addEventListener("keyup", (event) => {
    if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) update();
  });
  body.addEventListener(
    "keydown",
    (event) => {
      if (composing() || event.isComposing || event.ctrlKey || event.metaKey || event.altKey)
        return;
      if (["ArrowDown", "ArrowUp", "Enter", "Tab", "Escape"].includes(event.key)) {
        update();
        if (menu.hidden) return;
        event.preventDefault();
        event.stopImmediatePropagation();
        if (event.key === "Escape") {
          dismissed = `${body.value}\0${body.selectionStart}`;
          hide();
        } else if (event.key === "Enter" || event.key === "Tab") choose();
        else {
          selected =
            (selected + (event.key === "ArrowDown" ? 1 : options.length - 1)) % options.length;
          highlight();
        }
      }
    },
    true,
  );
  menu.addEventListener("pointerdown", (event) => event.preventDefault());
  body.addEventListener("blur", hide);
  body.addEventListener("compositionstart", hide);
  body.addEventListener("compositionend", () => requestAnimationFrame(update));
  document.addEventListener("tungsten:note", hide);
  document.addEventListener("tungsten:page", hide);
  window.addEventListener("resize", hide);
  return update;
}
