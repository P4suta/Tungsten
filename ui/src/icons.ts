const paths = {
  lock: "M5 10h14v11H5Zm3 0V7a4 4 0 0 1 8 0v3m-4 4v3",
  copy: "M9 8h12v13H9ZM3 16V3h12",
  home: "M3 10 12 3l9 7v11h-6v-7H9v7H3Z",
  add: "M12 4v16M4 12h16",
  more: "M4 12h2m5 0h2m5 0h2",
  close: "m6 6 12 12M18 6 6 18",
  settings: "M4 6h16M4 12h16M4 18h16M8 3v6m8 0v6m-6 0v6",
  search: "M10 3a7 7 0 1 0 0 14 7 7 0 0 0 0-14m5 12 6 6",
  edit: "m4 16 12-12 4 4L8 20H4Zm10-10 4 4",
  note: "M5 3h10l4 4v14H5Zm9 0v5h5M8 12h8m-8 4h8",
  folder: "M3 6h7l2 3h9v12H3Z",
  file: "M5 3h9l5 5v13H5Zm9 0v6h5",
  link: "m9 15 6-6M8 12l-3 3a3 3 0 0 0 4 4l3-3M16 12l3-3a3 3 0 0 0-4-4l-3 3",
  graph:
    "M9 5a2 2 0 1 1-4 0 2 2 0 0 1 4 0Zm10 10a2 2 0 1 1-4 0 2 2 0 0 1 4 0ZM9 19a2 2 0 1 1-4 0 2 2 0 0 1 4 0ZM8 7l7 6M7 7v10m2 1 6-2",
  upload: "M12 16V3m-5 5 5-5 5 5M4 16v5h16v-5",
  download: "M12 3v13m-5-5 5 5 5-5M4 17v4h16v-4",
  trash: "M3 6h18M9 6V3h6v3M6 6v15h12V6M10 10v7m4-7v7",
  undo: "M8 4 3 9l5 5M3 9h10a7 7 0 0 1 0 14",
  history: "M3 4v6h6M3 10a9 9 0 1 1 1 8m8-11v6l4 2",
  image: "M3 3h18v18H3Zm0 14 6-6 5 5 3-3 4 4M15 7h2",
  audio: "M9 18V5l11-2v13M4 16h5v5H4Zm11-2h5v5h-5",
  video: "M3 5h13v14H3Zm13 5 5-3v10l-5-3",
  format: "M4 4h16M12 4v16m-5 0h10M4 10h3m10 4h3",
  check: "m4 12 5 5L20 6",
  sort: "M7 3v18m-4-4 4 4 4-4M14 5h7m-7 6h5m-5 6h3",
  print: "M7 8V3h10v5M7 17H3V8h18v9h-4M7 14h10v7H7Z",
  table: "M3 3h18v18H3ZM3 9h18M3 15h18M9 3v18m6-18v18",
  list: "M3 5h2m4 0h12M3 12h2m4 0h12M3 19h2m4 0h12",
  collapse: "m6 9 6 6 6-6",
  expand: "m9 6 6 6-6 6",
  zoomin: "M10 3a7 7 0 1 0 0 14 7 7 0 0 0 0-14m5 12 6 6M7 10h6m-3-3v6",
  zoomout: "M10 3a7 7 0 1 0 0 14 7 7 0 0 0 0-14m5 12 6 6M7 10h6",
  fit: "M3 9V3h6m6 0h6v6m0 6v6h-6M9 21H3v-6",
  previous: "M21 12H3m7-7-7 7 7 7",
  next: "M3 12h18m-7-7 7 7-7 7",
} as const;
export type IconName = keyof typeof paths;

export function icon(name: IconName): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "-1 -1 26 26");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("focusable", "false");
  svg.classList.add("control-icon");
  svg.dataset.icon = name;
  const path = document.createElementNS(svg.namespaceURI, "path");
  path.setAttribute("d", paths[name]);
  svg.append(path);
  return svg;
}

function purpose(node: HTMLElement): IconName {
  if (
    node.matches("#notes button, #quick-results button, #orphan-notes button, #file-references a")
  )
    return "note";
  if (node.closest("#outgoing-links, #backlinks, #link-results")) return "link";
  if (node.closest("#outline")) return "list";
  if (node instanceof HTMLAnchorElement) {
    if (node.id === "show-notes") return "graph";
    if (node.download) return "download";
    if (node.dataset.page === "files") return "folder";
    if (node.dataset.page === "trash") return "trash";
    return "link";
  }
  const name =
    `${node.id} ${node.getAttribute("aria-label") ?? ""} ${node.textContent ?? ""}`.toLowerCase();
  for (const [pattern, kind] of [
    [/close|×/, "close"],
    [/home/, "home"],
    [/new|create/, "add"],
    [/more|options/, "more"],
    [/zoom-in|zoom in/, "zoomin"],
    [/zoom-out|zoom out/, "zoomout"],
    [/fit/, "fit"],
    [/overview/, "graph"],
    [/previous/, "previous"],
    [/next/, "next"],
    [/reset|restore|recover|retry|reload|repair/, "undo"],
    [/settings|font|width|size|height|spacing|appearance|spellcheck/, "settings"],
    [/search|find/, "search"],
    [/delete|trash/, "trash"],
    [/upload|import|attach-files/, "upload"],
    [/download|export/, "download"],
    [/history|version/, "history"],
    [/format|markdown|typing/, "format"],
    [/image/, "image"],
    [/audio/, "audio"],
    [/video/, "video"],
    [/print/, "print"],
    [/folder|files|library/, "folder"],
    [/link|connection|backlink|relation/, "link"],
    [/sort/, "sort"],
    [/table/, "table"],
    [/list|outline/, "list"],
    [/rename|replace|edit/, "edit"],
    [/save|apply|match|center/, "check"],
  ] satisfies [RegExp, IconName][])
    if (pattern.test(name)) return kind;
  return node instanceof HTMLAnchorElement ? "link" : "note";
}

export function initIcons(): void {
  function decorate(root: ParentNode): void {
    const selector = "button, a[href], summary, [role=option]";
    const controls = [...root.querySelectorAll<HTMLElement>(selector)];
    if (root instanceof HTMLElement && root.matches(selector)) controls.unshift(root);
    for (const target of controls) {
      if (
        target.querySelector("svg") ||
        (target.closest(".cm-content") && !target.closest(".live-block"))
      )
        continue;
      if (target.getAttribute("aria-label") === "Close") {
        target.replaceChildren(icon("close"));
        continue;
      }
      target.prepend(icon(purpose(target)));
    }
    for (const input of root.querySelectorAll<
      HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement
    >("input:not([type=hidden]):not([type=checkbox]):not([type=radio]), select, textarea")) {
      if (
        input.id === "body" ||
        input.hidden ||
        input.parentElement?.classList.contains("field-icon")
      )
        continue;
      const wrapper = document.createElement("div");
      wrapper.className = "field-icon";
      input.before(wrapper);
      wrapper.append(icon(purpose(input)), input);
    }
  }
  decorate(document);
  new MutationObserver((records) => {
    for (const record of records)
      for (const node of record.addedNodes) if (node instanceof HTMLElement) decorate(node);
  }).observe(document.body, {
    childList: true,
    subtree: true,
  });
}
