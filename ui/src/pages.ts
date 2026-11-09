import { focusWriting } from "./surface.js";
import { noteTitle } from "./titles.js";

export type PageName = "note" | "trash";

let paperScroll = 0;
let returnedToPaper = false;

export function returningFromPage(): boolean {
  return returnedToPaper;
}

export function currentPage(): PageName {
  if (location.pathname === "/trash") return "trash";
  return "note";
}

function renderPage(focus: boolean): void {
  const page = currentPage();
  returnedToPaper = document.body.dataset.page !== "note" && page === "note";
  if (document.body.dataset.page === "note" && page !== "note") paperScroll = scrollY;
  document.body.dataset.page = page;
  for (const [id, name] of [
    ["editor-pane", "note"],
    ["trash-page", "trash"],
  ] as const) {
    const element = document.getElementById(id);
    if (element) element.hidden = name !== page;
  }
  const actions = document.getElementById("paper-actions");
  if (actions) actions.hidden = page !== "note";
  for (const link of document.querySelectorAll<HTMLAnchorElement>("[data-page]")) {
    if (link.dataset.page === page) link.setAttribute("aria-current", "page");
    else link.removeAttribute("aria-current");
  }
  document.title =
    page === "note"
      ? noteTitle(document.querySelector<HTMLTextAreaElement>("#body")?.value ?? "")
      : "Trash";
  if (focus) {
    if (page === "note") {
      requestAnimationFrame(() => {
        const target = document.getElementById("body");
        if (target instanceof HTMLTextAreaElement) focusWriting(target);
        else target?.focus({ preventScroll: true });
        window.scrollTo({ top: paperScroll, behavior: "instant" });
      });
    } else {
      document.querySelector<HTMLElement>(`#${page}-page h1`)?.focus({ preventScroll: true });
      window.scrollTo({ top: 0, behavior: "instant" });
    }
  }
  document.dispatchEvent(new CustomEvent<PageName>("tungsten:page", { detail: page }));
}

export function openPage(page: PageName, replace = false, note?: string): void {
  for (const dialog of document.querySelectorAll<HTMLDialogElement>("dialog[open]")) dialog.close();
  const target = new URL(location.href);
  target.pathname = page === "note" ? "/" : `/${page}`;
  target.searchParams.delete("folder");
  if (note !== undefined) target.searchParams.set("note", note);
  if (target.href !== location.href) {
    const address = target.pathname + target.search + target.hash;
    if (replace) history.replaceState(null, "", address);
    else history.pushState(null, "", address);
  }
  renderPage(true);
}

export function closePage(note?: string): void {
  if (currentPage() !== "note") openPage("note", false, note);
}

export function initPages(): void {
  document.addEventListener("click", (event) => {
    if (!(event.target instanceof Element)) return;
    const link = event.target.closest<HTMLAnchorElement>("a[data-page]");
    if (!link || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
    const page = link.dataset.page;
    if (page === "trash" || page === "note") {
      event.preventDefault();
      openPage(page);
    }
  });
  for (const close of document.querySelectorAll<HTMLButtonElement>("[data-close-page]"))
    close.addEventListener("click", () => closePage());
  document.addEventListener("keydown", (event) => {
    if (
      event.key === "Escape" &&
      currentPage() !== "note" &&
      !document.querySelector("dialog[open]")
    ) {
      event.preventDefault();
      closePage();
    }
  });
  document.addEventListener("tungsten:note", () => {
    paperScroll = 0;
  });
  window.addEventListener("popstate", () => renderPage(true));
  renderPage(false);
}
