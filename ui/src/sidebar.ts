import { openDialog } from "./dialogs.js";
import {
  closeSidebar,
  type SidebarLayout,
  type SidebarSide,
  toggleSidebar,
  visiblePanes,
} from "./sidebar-layout.js";

export type LibraryView = "notes" | "trash";
export type ContextView = "links" | "history";

export interface SidebarController {
  navigation(view?: LibraryView): void;
  context(view?: ContextView): void;
  writing(): void;
  libraryView(): LibraryView;
  contextView(): ContextView;
  visible(side: SidebarSide): boolean;
  subscribe(listener: () => void): void;
}

export function navigationHasFocus(): boolean {
  return ["sidebar", "note-sidebar"].some((id) =>
    document.getElementById(id)?.contains(document.activeElement),
  );
}

function control<T extends HTMLElement>(id: string, kind: { new (): T }): T {
  const node = document.getElementById(id);
  if (!(node instanceof kind)) throw new Error(`Missing control: ${id}`);
  return node;
}

function read(storage: "localStorage" | "sessionStorage", key: string): string | null {
  try {
    return window[storage].getItem(key);
  } catch {
    return null;
  }
}

function remember(storage: "localStorage" | "sessionStorage", key: string, value: string): void {
  try {
    window[storage].setItem(key, value);
  } catch {
    return;
  }
}

export function initSidebar(): SidebarController {
  const left = control("sidebar", HTMLElement);
  const right = control("note-sidebar", HTMLDialogElement);
  const paper = control("paper-scroll", HTMLDivElement);
  const navigation = control("toggle-sidebar", HTMLButtonElement);
  const context = control("toggle-context", HTMLButtonElement);
  const write = control("show-paper", HTMLButtonElement);
  const wide = matchMedia("(min-width: 64rem)");
  let layout: SidebarLayout = {
    navigation: read("localStorage", "tungsten.navigation.visible") !== "closed",
    context: false,
    compact: "note",
  };
  let libraryView: LibraryView =
    read("sessionStorage", "tungsten.library.view") === "trash" ? "trash" : "notes";
  let contextView: ContextView =
    read("sessionStorage", "tungsten.context.view") === "history" ? "history" : "links";
  const listeners = new Set<() => void>();
  document.addEventListener("focusin", (event) => {
    if (!wide.matches || !(event.target instanceof Node)) return;
    const target = event.target;
    if (paper.contains(target)) layout = { ...layout, compact: "note" };
    else if (left.contains(target)) layout = { ...layout, compact: "navigation" };
  });

  if (location.pathname === "/trash") {
    libraryView = "trash";
    layout = { ...layout, navigation: true, compact: "navigation" };
    history.replaceState(null, "", `/${location.search}${location.hash}`);
  }

  function render(): void {
    const visibility = visiblePanes(layout, wide.matches);
    const active = document.activeElement;
    left.hidden = !visibility.navigation;
    if (visibility.context && !right.open) {
      openDialog(right, context);
      control("details-heading", HTMLHeadingElement).focus();
    } else if (!visibility.context && right.open) right.close();
    paper.hidden = !visibility.note;
    paper.inert = !visibility.note;
    navigation.setAttribute("aria-expanded", String(visibility.navigation));
    write.hidden = visibility.note;
    document.body.dataset.layout = wide.matches ? "wide" : "compact";
    document.body.dataset.navigation = visibility.navigation ? "open" : "closed";
    document.body.dataset.context = visibility.context ? "open" : "closed";
    control("library-pane", HTMLElement).hidden = libraryView !== "notes";
    control("trash-pane", HTMLElement).hidden = libraryView !== "trash";
    const locations = document.querySelector<HTMLElement>(".library-locations");
    if (locations) locations.hidden = libraryView === "trash";
    control("trash", HTMLButtonElement).hidden = libraryView === "trash";
    control("context-links", HTMLElement).hidden = contextView !== "links";
    control("history-panel", HTMLElement).hidden = contextView !== "history";
    control("trash", HTMLButtonElement).setAttribute(
      "aria-pressed",
      String(libraryView === "trash"),
    );
    control("context-links-tab", HTMLButtonElement).setAttribute(
      "aria-pressed",
      String(contextView === "links"),
    );
    control("history", HTMLButtonElement).setAttribute(
      "aria-pressed",
      String(contextView === "history"),
    );
    if (active && left.hidden && left.contains(active)) navigation.focus({ preventScroll: true });
    for (const listener of listeners) listener();
  }

  function persist(): void {
    remember("localStorage", "tungsten.navigation.visible", layout.navigation ? "open" : "closed");
    remember("sessionStorage", "tungsten.library.view", libraryView);
    remember("sessionStorage", "tungsten.context.view", contextView);
  }

  function toggle(side: SidebarSide): void {
    layout = toggleSidebar(layout, side, wide.matches);
    persist();
    render();
    if (side === "navigation" && visiblePanes(layout, wide.matches)[side])
      control(side === "navigation" ? "close-sidebar" : "close-context", HTMLButtonElement).focus({
        preventScroll: true,
      });
  }

  function close(side: SidebarSide): void {
    layout = closeSidebar(layout, side, wide.matches);
    persist();
    render();
    (side === "navigation" ? navigation : context).focus({ preventScroll: true });
  }

  const controller: SidebarController = {
    navigation(view) {
      if (view) libraryView = view;
      layout = { ...layout, navigation: true, compact: "navigation" };
      persist();
      render();
    },
    context(view) {
      if (view) contextView = view;
      layout = { ...layout, context: true };
      persist();
      render();
    },
    writing() {
      layout = { ...layout, compact: "note", context: false };
      render();
    },
    libraryView: () => libraryView,
    contextView: () => contextView,
    visible: (side) => visiblePanes(layout, wide.matches)[side],
    subscribe: (listener) => listeners.add(listener),
  };
  navigation.disabled = false;
  context.disabled = false;
  navigation.addEventListener("click", () => toggle("navigation"));
  context.addEventListener("click", () => toggle("context"));
  control("close-sidebar", HTMLButtonElement).addEventListener("click", () => close("navigation"));
  control("close-context", HTMLButtonElement).addEventListener("click", () => close("context"));
  right.addEventListener("close", () => {
    layout = { ...layout, context: false };
    render();
  });
  control("trash", HTMLButtonElement).addEventListener("click", () =>
    controller.navigation("trash"),
  );
  control("trash-back", HTMLButtonElement).addEventListener("click", () =>
    controller.navigation("notes"),
  );
  control("context-links-tab", HTMLButtonElement).addEventListener("click", () =>
    controller.context("links"),
  );
  control("history", HTMLButtonElement).addEventListener("click", () =>
    controller.context("history"),
  );
  write.addEventListener("click", () => {
    controller.writing();
    document.dispatchEvent(new Event("tungsten:write"));
  });
  document.querySelector<HTMLAnchorElement>(".skip")?.addEventListener("click", (event) => {
    event.preventDefault();
    controller.writing();
    document.dispatchEvent(new Event("tungsten:write"));
  });
  document.addEventListener("keydown", (event) => {
    if (event.key !== "Escape" || event.isComposing || document.querySelector("dialog[open]"))
      return;
    const target = event.target;
    if (!(target instanceof Node)) return;
    if (!left.hidden && (left.contains(target) || !wide.matches)) {
      event.preventDefault();
      close("navigation");
    } else if (!right.hidden && (right.contains(target) || !wide.matches)) {
      event.preventDefault();
      close("context");
    }
  });
  wide.addEventListener("change", render);
  render();
  return controller;
}
