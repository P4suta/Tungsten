import { type Summary, summaries } from "./catalog.js";
import { initContext } from "./context.js";
import { initDialogChrome, openDialog } from "./dialogs.js";

export { type Summary, summaries } from "./catalog.js";

import { initFileLinkGuard } from "./file-link-guard.js";
import { removeFileLinks } from "./file-links.js";
import { formatMarkdown, mappedCursor } from "./formatting.js";
import { initHome } from "./home.js";
import { initIcons } from "./icons.js";
import { initInput, wrapSelection } from "./input.js";
import { initLibrary } from "./library.js";
import { noteLink, noteLinkRange } from "./links.js";
import { initLive, type RenderedBlock } from "./live.js";
import { type Attachment, attachmentMarkdown, parseAttachment } from "./media.js";
import { withinPreviewBudget } from "./rendering.js";
import { initSidebar } from "./sidebar.js";
import { initInsertions } from "./slash.js";
import { focusWriting, selectWriting, writingElement } from "./surface.js";
import { noteTitle } from "./titles.js";
import { initTrash } from "./trash.js";
import { initWriting } from "./writing.js";

export interface Snapshot {
  identity: string;
  name: string | null;
  title: string;
  body: string;
  savedBody: string;
  revision: string | null;
  busy: boolean;
  composing: boolean;
}

export interface EditorPort {
  state(): Snapshot | null;
  api(path: string, method?: string, input?: unknown, revision?: string | null): Promise<unknown>;
  open(name: string): Promise<void>;
  create(): Promise<string | null>;
  blank(title: string, body: string): void;
  mutate(action: (note: Snapshot) => Promise<void>): Promise<boolean>;
  refresh(): Promise<Summary[]>;
}

interface InsertionTarget {
  identity: string;
  body: string;
  start: number;
  end: number;
}

interface Rendered {
  html: string;
  blocks: RenderedBlock[];
  tags: string[];
  links: string[];
  outline: { id: string; text: string; level: number }[];
  tasks: { offset: number; checked: boolean }[];
}

export interface FeatureController {
  open(name: string): Promise<void>;
  changed(): void;
  indexed(): Promise<void>;
  filter(notes: Summary[]): Summary[];
  write(): void;
  anchor(fragment: string): Promise<void>;
  beforeSave(identity: string, text: string): Promise<void>;
  home(): Promise<void>;
  searchResults(notes: Summary[]): void;
}

function control<T extends HTMLElement>(id: string, kind: { new (): T }): T {
  const node = document.getElementById(id);
  if (!(node instanceof kind)) throw new Error(`Missing control: ${id}`);
  return node;
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function strings(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item: unknown) => typeof item === "string");
}

function rendered(value: unknown): Rendered {
  if (
    !record(value) ||
    typeof value.html !== "string" ||
    !strings(value.tags) ||
    !strings(value.links) ||
    !Array.isArray(value.outline) ||
    !Array.isArray(value.tasks)
  )
    throw new Error("The preview data is invalid.");
  const outline = value.outline.map((entry: unknown) => {
    if (
      !record(entry) ||
      typeof entry.id !== "string" ||
      typeof entry.text !== "string" ||
      typeof entry.level !== "number"
    )
      throw new Error("The outline data is invalid.");
    return { id: entry.id, text: entry.text, level: entry.level };
  });
  const tasks = value.tasks.map((entry: unknown) => {
    if (
      !record(entry) ||
      typeof entry.offset !== "number" ||
      !Number.isSafeInteger(entry.offset) ||
      entry.offset < 0 ||
      typeof entry.checked !== "boolean"
    )
      throw new Error("The task data is invalid.");
    return { offset: entry.offset, checked: entry.checked };
  });
  if (!Array.isArray(value.blocks)) throw new Error("The rendered blocks are invalid.");
  const blocks = value.blocks.map((entry: unknown): RenderedBlock => {
    if (
      !record(entry) ||
      typeof entry.from !== "number" ||
      typeof entry.to !== "number" ||
      !Number.isSafeInteger(entry.from) ||
      !Number.isSafeInteger(entry.to) ||
      entry.from < 0 ||
      entry.to < entry.from ||
      typeof entry.html !== "string"
    )
      throw new Error("The rendered block is invalid.");
    return { from: entry.from, to: entry.to, html: entry.html };
  });
  return { html: value.html, blocks, tags: value.tags, links: value.links, outline, tasks };
}

function button(text: string, action: () => void): HTMLButtonElement {
  const node = document.createElement("button");
  node.type = "button";
  node.textContent = text;
  node.addEventListener("click", action);
  return node;
}

export function initFeatures(port: EditorPort): FeatureController {
  initDialogChrome();
  initIcons();
  const sidebar = initSidebar();
  const body = control("body", HTMLTextAreaElement);
  const previewStatus = control("preview-status", HTMLParagraphElement);
  const notice = control("feature-status", HTMLParagraphElement);
  const switcher = control("switcher-dialog", HTMLDialogElement);
  const quickSearch = control("quick-search", HTMLInputElement);
  const quickList = control("quick-results", HTMLUListElement);
  const fileLinks = initFileLinkGuard({ state: port.state, api: port.api, replace: replaceBody });
  const home = initHome({ state: port.state, api: port.api, navigate, blank: port.blank });
  initTrash({ api: port.api, navigate, refresh: port.refresh, sidebar });
  const library = initLibrary({
    state: port.state,
    api: port.api,
    open: navigate,
    create: port.create,
    sidebar,
  });
  const context = initContext({ ...port, navigate, sidebar });
  let notes: Summary[] = [];
  let indexGeneration = 0;
  let previewGeneration = 0;
  let previewTimer: number | null = null;
  let choosingFile = false;
  let renderedState: Snapshot | null = null;
  let renderedTasks: Rendered["tasks"] = [];
  let lastIdentity = "";
  let lastText = "";
  let lastName: string | null = null;
  let formatIdentity: string | null = null;
  const live = initLive(body, control("live-editor", HTMLDivElement), {
    api: (path) => port.api(path),
    help: () => ({ spellcheck: true, tabSize: 4 }),
    attach: (files) => {
      void run(() => attach(files));
    },
  });
  initInput(
    body,
    () => !body.disabled && !body.readOnly && !port.state()?.composing,
    () => true,
    () => 4,
  );
  initWriting(
    body,
    () => true,
    () => port.state()?.composing ?? false,
  );

  function report(text: string, error = false): void {
    notice.setAttribute("role", error ? "alert" : "status");
    notice.textContent = text;
    notice.hidden = text === "";
    notice.classList.toggle("visually-hidden", !error);
  }

  async function run(action: () => Promise<void>, target?: HTMLElement): Promise<void> {
    try {
      await action();
    } catch (error) {
      const message = error instanceof Error ? error.message : "The operation failed.";
      if (target) target.textContent = message;
      else report(message, true);
    }
  }

  function insert(
    text: string,
    start = body.selectionStart,
    end = body.selectionEnd,
    select = false,
  ): void {
    const state = port.state();
    if (!state || state.composing || body.disabled || body.readOnly) return;
    sidebar.writing();
    body.setRangeText(text, start, end, select ? "select" : "end");
    body.dispatchEvent(new Event("input", { bubbles: true }));
    focusWriting(body);
  }

  function replaceBody(text: string): void {
    insert(text, 0, body.value.length);
  }

  document.addEventListener("tungsten:forget-file", (event) => {
    if (!(event instanceof CustomEvent)) return;
    void run(async () => {
      const file = parseAttachment(event.detail);
      const identity = port.state()?.identity;
      if (
        !confirm(
          `Permanently delete ${file.name}? The original file will also become unavailable from note history and other notes.`,
        )
      )
        return;
      await port.api(`/api/attachments/${file.id}`, "DELETE", undefined, file.revision);
      if (port.state()?.identity === identity) {
        replaceBody(removeFileLinks(body.value, file.id));
        await loadPreview();
      }
      report("Original file deleted permanently.");
    });
  });

  let renderedOutline: Rendered["outline"] = [];

  async function anchor(fragment: string): Promise<void> {
    let id: string;
    try {
      id = decodeURIComponent(fragment.replace(/^#/, ""));
    } catch {
      return;
    }
    sidebar.writing();
    await loadPreview();
    const index = renderedOutline.findIndex(
      (heading) => heading.id === id || heading.id === `md-${id}`,
    );
    if (index < 0) return;
    live.anchor(index);
    const url = new URL(location.href);
    url.hash = renderedOutline[index]?.id ?? "";
    window.history.replaceState(null, "", url);
  }

  async function navigate(name: string): Promise<void> {
    try {
      await port.open(name);
      sidebar.writing();
      focusWriting(body);
    } catch (error) {
      if (record(error) && error.status === 404) throw new Error("The linked note is unavailable.");
      throw error;
    }
  }

  function relatedList(id: string, names: string[], action: (name: string) => void): void {
    const list = control(id, HTMLUListElement);
    list.replaceChildren();
    for (const name of names) {
      const item = document.createElement("li");
      const title = notes.find((note) => note.name === name)?.title;
      item.append(
        button(title || noteTitle(notes.find((note) => note.name === name)?.snippet ?? ""), () =>
          action(name),
        ),
      );
      list.append(item);
    }
    if (names.length === 0) {
      const item = document.createElement("li");
      item.textContent = "None";
      list.append(item);
    }
  }

  async function loadPreview(): Promise<void> {
    if (previewTimer !== null) {
      window.clearTimeout(previewTimer);
      previewTimer = null;
    }
    const state = port.state();
    if (!state) return;
    const generation = ++previewGeneration;
    if (!withinPreviewBudget(state.body)) {
      live.render(state.body, []);
      renderedOutline = [];
      renderedState = state;
      renderedTasks = [];
      previewStatus.textContent = "Large note: displaying Markdown source.";
      return;
    }
    previewStatus.textContent = "Loading preview";
    try {
      const data = rendered(
        await port.api("/api/preview", "POST", {
          name:
            state.name ??
            (state.title ? `${state.title}${state.title.endsWith(".md") ? "" : ".md"}` : ""),
          body: state.body,
        }),
      );
      const current = port.state();
      if (
        generation !== previewGeneration ||
        current?.identity !== state.identity ||
        current.body !== state.body
      )
        return;
      live.render(state.body, data.blocks);
      renderedOutline = data.outline;
      renderedState = state;
      renderedTasks = data.tasks;
      previewStatus.textContent = "";
      updateTaskAvailability();
    } catch (error) {
      if (generation === previewGeneration) previewStatus.textContent = "Preview unavailable.";
      throw error;
    }
  }

  function updateTaskAvailability(): void {
    const state = port.state();
    for (const checkbox of document.querySelectorAll<HTMLInputElement>(
      "#live-editor input[data-task]",
    ))
      checkbox.disabled =
        !state ||
        state.busy ||
        state.composing ||
        state.identity !== renderedState?.identity ||
        state.body !== renderedState.body;
  }

  function changeTask(event: Event): void {
    if (!(event.target instanceof HTMLInputElement)) return;
    const task = renderedTasks[Number(event.target.dataset.task)];
    const state = port.state();
    if (
      !task ||
      !state ||
      state.identity !== renderedState?.identity ||
      state.body !== renderedState.body ||
      state.busy ||
      state.composing ||
      !/^\[[ xX]\]$/.test(state.body.slice(task.offset, task.offset + 3))
    )
      return;
    body.setRangeText(
      event.target.checked ? "[x]" : "[ ]",
      task.offset,
      task.offset + 3,
      "preserve",
    );
    body.dispatchEvent(new Event("input", { bubbles: true }));
    void run(loadPreview);
  }
  control("live-editor", HTMLElement).addEventListener("change", changeTask);

  function followLink(event: MouseEvent): void {
    if (!(event.target instanceof Element)) return;
    const link = event.target.closest("a");
    if (
      !link ||
      event.defaultPrevented ||
      (event instanceof MouseEvent &&
        (event.ctrlKey || event.metaKey || event.shiftKey || event.altKey))
    )
      return;
    const raw = link.getAttribute("href") ?? "";
    if (raw.startsWith("#")) {
      event.preventDefault();
      void run(() => anchor(raw));
      return;
    }
    const url = new URL(link.href);
    const name = url.origin === location.origin ? url.searchParams.get("note") : null;
    if (name) {
      event.preventDefault();
      void run(async () => {
        if (port.state()?.name !== name) await navigate(name);
        if (url.hash && port.state()?.name === name) {
          await anchor(url.hash);
        }
      });
    }
  }
  control("live-editor", HTMLElement).addEventListener("click", followLink);

  async function indexed(): Promise<void> {
    const generation = ++indexGeneration;
    const result = summaries(await port.api("/api/notes"));
    if (generation !== indexGeneration) return;
    notes = filter(result);
    quickResults();
    refreshCompletion();
  }

  function filter(entries: Summary[]): Summary[] {
    return [...entries].sort((a, b) => b.modified - a.modified || a.title.localeCompare(b.title));
  }

  function changed(): void {
    const state = port.state();
    if (!state) return;
    fileLinks.changed(state);
    library.changed(state);
    context.changed(state);
    live.sync(state.identity);
    const identityChanged = state.identity !== lastIdentity;
    const changed = identityChanged || state.body !== lastText || state.name !== lastName;
    if (identityChanged) {
      formatIdentity = state.body !== state.savedBody ? state.identity : null;
      renderedState = null;
      renderedTasks = [];
      renderedOutline = [];
      report("");
    }
    lastIdentity = state.identity;
    lastText = state.body;
    lastName = state.name;
    updateTaskAvailability();
    if (changed) {
      ++previewGeneration;
      if (previewTimer !== null) window.clearTimeout(previewTimer);
      if (identityChanged) void run(loadPreview);
      else
        previewTimer = window.setTimeout(() => {
          void run(loadPreview);
        }, 250);
    }
  }

  function format(kind: string): void {
    const edit = wrapSelection(
      { text: body.value, start: body.selectionStart, end: body.selectionEnd },
      kind === "bold" ? "**" : "*",
    );
    replaceBody(edit.text);
    selectWriting(body, edit.start, edit.end);
  }

  async function tidy(focus: boolean): Promise<void> {
    const state = port.state();
    if (
      !state ||
      state.composing ||
      (!focus && formatIdentity !== state.identity) ||
      choosingFile ||
      body.disabled ||
      body.readOnly ||
      !body.value.trim() ||
      /(?:^|\n) *\/[a-z]*$/.test(body.value)
    )
      return;
    const before = body.value;
    const start = body.selectionStart;
    const end = body.selectionEnd;
    const after = await formatMarkdown(before);
    const current = port.state();
    if (
      current?.identity !== state.identity ||
      current.composing ||
      body.value !== before ||
      body.selectionStart !== start ||
      body.selectionEnd !== end ||
      (!focus && writingElement(body).contains(document.activeElement)) ||
      body.readOnly ||
      body.disabled
    )
      return;
    if (after !== before) {
      body.value = after;
      body.setSelectionRange(mappedCursor(before, after, start), mappedCursor(before, after, end));
      body.dispatchEvent(new Event("input", { bubbles: true }));
    }
    formatIdentity = null;
    if (focus) {
      sidebar.writing();
      focusWriting(body);
    }
  }
  body.addEventListener("input", () => {
    formatIdentity = port.state()?.identity ?? null;
  });
  body.addEventListener("blur", () => {
    if (!port.state()?.composing) void run(() => tidy(false));
  });
  document.addEventListener("keydown", (event) => {
    if (event.altKey && event.shiftKey && event.key.toLowerCase() === "f" && !event.isComposing) {
      event.preventDefault();
      void run(() => tidy(true));
    }
  });

  let quickGeneration = 0;
  let quickTimer: number | null = null;
  let renderedQuickResults: string | null = null;
  function clearQuickResults(): void {
    ++quickGeneration;
    renderedQuickResults = null;
    quickList.replaceChildren();
    quickList.setAttribute("aria-busy", "true");
  }
  async function quickResults(): Promise<void> {
    if (quickTimer !== null) window.clearTimeout(quickTimer);
    quickTimer = null;
    const generation = ++quickGeneration;
    const needle = quickSearch.value.toLocaleLowerCase();
    quickList.setAttribute("aria-busy", "true");
    try {
      const result = summaries(
        await port.api(`/api/notes?titles=true&limit=50&q=${encodeURIComponent(needle)}`),
      );
      if (generation !== quickGeneration || quickSearch.value.toLocaleLowerCase() !== needle)
        return;
      notes = result;
      const visible = notes
        .filter((note) =>
          (note.title || noteTitle(note.snippet)).toLocaleLowerCase().includes(needle),
        )
        .slice(0, 50);
      const rendered = JSON.stringify(
        visible.map((note) => [note.name, note.title || noteTitle(note.snippet)]),
      );
      if (rendered !== renderedQuickResults) {
        relatedList(
          "quick-results",
          visible.map((note) => note.name),
          (name) => {
            switcher.close();
            void run(() => navigate(name));
          },
        );
        renderedQuickResults = rendered;
      }
    } catch (error) {
      if (generation === quickGeneration)
        report(error instanceof Error ? error.message : "Search is unavailable.", true);
    } finally {
      if (generation === quickGeneration) quickList.setAttribute("aria-busy", "false");
    }
  }
  quickSearch.addEventListener("input", () => {
    clearQuickResults();
    if (quickTimer !== null) window.clearTimeout(quickTimer);
    quickTimer = window.setTimeout(() => {
      void quickResults();
    }, 120);
  });
  quickSearch.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      control("quick-results", HTMLUListElement).querySelector("button")?.focus();
    }
  });
  for (const close of document.querySelectorAll<HTMLButtonElement>("[data-close]"))
    close.addEventListener("click", () => close.closest("dialog")?.close());

  function insertionTarget(
    start = body.selectionStart,
    end = body.selectionEnd,
  ): InsertionTarget | null {
    const state = port.state();
    if (!state || state.composing || body.readOnly || body.disabled) return null;
    return { identity: state.identity, body: body.value, start, end };
  }

  const pendingAttachments = new Map<string, Attachment[]>();
  function placeAttachments(target: InsertionTarget, uploaded: Attachment[]): void {
    const state = port.state();
    if (!state || state.identity !== target.identity || state.composing || body.readOnly) {
      pendingAttachments.set(target.identity, [
        ...(pendingAttachments.get(target.identity) ?? []),
        ...uploaded,
      ]);
      report("Files are ready for the original note. Return to the note to insert them.", true);
      return;
    }
    const markdown = uploaded
      .map((file) => attachmentMarkdown(file, state.name ?? state.title))
      .join("\n\n");
    const unchanged = body.value === target.body;
    insert(
      unchanged ? markdown : `\n\n${markdown}`,
      unchanged ? target.start : body.value.length,
      unchanged ? target.end : body.value.length,
    );
    report(`Attached ${uploaded.length} ${uploaded.length === 1 ? "file" : "files"}`);
  }
  async function attach(files: File[], target = insertionTarget()): Promise<void> {
    if (!files.length || !target) return;
    const uploaded: Attachment[] = [];
    try {
      for (const file of files) {
        if (file.size > 64 * 1024 * 1024) throw new Error("Attachments must be at most 64 MiB.");
        report(`Uploading ${file.name}`);
        uploaded.push(parseAttachment(await port.api("/api/attachments", "POST", file)));
      }
    } finally {
      if (uploaded.length) placeAttachments(target, uploaded);
    }
  }
  function flushAttachments(): void {
    const target = insertionTarget(body.value.length, body.value.length);
    if (!target) return;
    const uploaded = pendingAttachments.get(target.identity);
    if (!uploaded) return;
    pendingAttachments.delete(target.identity);
    placeAttachments(target, uploaded);
  }
  document.addEventListener("tungsten:note", flushAttachments);
  body.addEventListener("compositionend", flushAttachments);

  const slashFiles = control("slash-files", HTMLInputElement);
  let slashTarget: InsertionTarget | null = null;
  let completion: { key: string; notes: Summary[] } | null = null;
  let completionKey = "";
  let completionTimer: number | null = null;
  const refreshCompletion = initInsertions(
    body,
    () => port.state()?.composing ?? false,
    (command, range) => {
      if (command.kind === "block") {
        insert(command.text, range.start, range.end);
        selectWriting(body, range.start + command.cursor, range.start + command.cursor);
      } else {
        slashTarget = insertionTarget(range.start, range.end);
        if (!slashTarget) return;
        slashFiles.accept = command.accept;
        slashFiles.value = "";
        choosingFile = true;
        slashFiles.click();
      }
    },
    (text, start, end) => {
      const range = noteLinkRange(text, start, end);
      const current = port.state();
      if (!range || !current) return null;
      const key = `${current.identity}\0${range.query}`;
      if (completionKey !== key) {
        completionKey = key;
        if (completionTimer !== null) window.clearTimeout(completionTimer);
        completionTimer = window.setTimeout(() => {
          void port
            .api(`/api/notes?titles=true&limit=50&q=${encodeURIComponent(range.query)}`)
            .then((value) => {
              if (completionKey !== key || port.state()?.identity !== current.identity) return;
              completion = { key, notes: summaries(value) };
              refreshCompletion();
            })
            .catch(() => {});
        }, 100);
      }
      return {
        range,
        commands: (completion?.key === key ? completion.notes : [])
          .filter(
            (note) =>
              note.name !== current.name &&
              (note.title || noteTitle(note.snippet)).toLocaleLowerCase().includes(range.query),
          )
          .slice(0, 12)
          .map((note) => {
            const label = note.title || noteTitle(note.snippet);
            const text = noteLink(current.name ?? "note.md", note.name, label);
            return { id: note.name, label, kind: "block" as const, text, cursor: text.length };
          }),
      };
    },
  );
  slashFiles.addEventListener("change", () => {
    choosingFile = false;
    const files = Array.from(slashFiles.files ?? []);
    const target = slashTarget;
    slashTarget = null;
    slashFiles.value = "";
    if (target && files.length) void run(() => attach(files, target));
    focusWriting(body);
  });
  slashFiles.addEventListener("cancel", () => {
    choosingFile = false;
    focusWriting(body);
  });
  body.addEventListener("paste", (event) => {
    const files = Array.from(event.clipboardData?.files ?? []);
    if (files.length) {
      event.preventDefault();
      void run(() => attach(files));
    }
  });
  body.addEventListener("dragover", (event) => {
    if (event.dataTransfer?.types.includes("Files")) event.preventDefault();
  });
  body.addEventListener("drop", (event) => {
    const files = Array.from(event.dataTransfer?.files ?? []);
    if (files.length) {
      event.preventDefault();
      void run(() => attach(files));
    }
  });

  window.addEventListener("offline", () =>
    report("Offline: keep this tab open until the note is saved."),
  );

  document.addEventListener("keydown", (event) => {
    const state = port.state();
    if (
      event.isComposing ||
      state?.composing ||
      !(event.ctrlKey || event.metaKey) ||
      event.altKey ||
      document.querySelector("dialog[open]")
    )
      return;
    const key = event.key.toLowerCase();
    if (key === "p" && !event.shiftKey) {
      event.preventDefault();
      quickSearch.value = "";
      clearQuickResults();
      quickResults();
      openDialog(switcher);
      quickSearch.focus();
      void run(indexed);
    } else if (key === "n") {
      event.preventDefault();
      control("new-note", HTMLButtonElement).click();
    } else if (key === "f" && event.shiftKey) {
      event.preventDefault();
      openDialog(control("notes-dialog", HTMLDialogElement));
      control("search", HTMLInputElement).focus();
      void home.open();
    } else if (event.target === body && !event.shiftKey && (key === "b" || key === "i")) {
      event.preventDefault();
      format(key === "b" ? "bold" : "italic");
    }
  });

  live.show();
  document.addEventListener("tungsten:write", () => {
    live.show();
    focusWriting(body);
  });
  void run(loadPreview);
  return {
    open: navigate,
    changed,
    indexed,
    filter,
    write: () => {
      sidebar.writing();
      live.show();
    },
    anchor,
    beforeSave: fileLinks.beforeSave,
    home: home.open,
    searchResults: home.search,
  };
}
