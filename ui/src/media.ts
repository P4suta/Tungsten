import type { PDFDocumentProxy, RenderTask } from "./pdf.mjs";

export interface Attachment {
  id: string;
  name: string;
  revision: string;
  bytes: number;
  url: string;
  image: boolean;
  kind: "image" | "audio" | "video" | "pdf" | "file";
  mime: string;
  folder: string;
  description: string;
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export function parseAttachment(value: unknown): Attachment {
  if (
    !record(value) ||
    typeof value.id !== "string" ||
    !/^[a-f0-9]{32}\.[a-zA-Z0-9]{1,12}$/.test(value.id) ||
    typeof value.name !== "string" ||
    typeof value.revision !== "string" ||
    typeof value.bytes !== "number" ||
    !Number.isSafeInteger(value.bytes) ||
    value.bytes < 0 ||
    typeof value.image !== "boolean" ||
    value.url !== `/.attachments/${value.id}` ||
    typeof value.mime !== "string" ||
    typeof value.folder !== "string" ||
    typeof value.description !== "string"
  )
    throw new Error("The attached file data is invalid.");
  const kind = value.kind;
  if (kind !== "image" && kind !== "audio" && kind !== "video" && kind !== "pdf" && kind !== "file")
    throw new Error("The attached file type is invalid.");
  return {
    id: value.id,
    name: value.name,
    revision: value.revision,
    bytes: value.bytes,
    url: value.url,
    image: value.image,
    kind,
    mime: value.mime,
    folder: value.folder,
    description: value.description,
  };
}

export function attachmentMarkdown(file: Attachment, note: string): string {
  const depth = note.split("/").length - 1;
  return `${file.image ? "!" : ""}[${file.name.replace(/[\\[\]]/g, "\\$&")}](${"../".repeat(depth)}.attachments/${file.id})`;
}

function action(text: string, run: () => void): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = text;
  button.addEventListener("click", run);
  return button;
}

function download(file: Attachment): HTMLAnchorElement {
  const link = document.createElement("a");
  link.href = `${file.url}?download=1`;
  link.download = file.name;
  link.textContent = `Download ${file.name}`;
  link.className = "control";
  return link;
}

function fileOptions(file: Attachment): HTMLDetailsElement {
  const details = document.createElement("details");
  const summary = document.createElement("summary");
  summary.textContent = "File options";
  const remove = action("Delete file permanently", () => {
    details.dispatchEvent(
      new CustomEvent<Attachment>("tungsten:forget-file", { bubbles: true, detail: file }),
    );
  });
  details.append(summary, remove);
  return details;
}

function pdfView(host: HTMLElement, file: Attachment, pdf: typeof import("./pdf.mjs")): () => void {
  const { GlobalWorkerOptions, getDocument } = pdf;
  let disposed = false;
  let document: PDFDocumentProxy | null = null;
  let rendering: RenderTask | null = null;
  let generation = 0;
  let number = 1;
  let zoom = 1;
  const owner = host.ownerDocument;
  const status = owner.createElement("p");
  status.setAttribute("role", "status");
  status.textContent = "Loading PDF";
  const toolbar = owner.createElement("div");
  toolbar.className = "actions";
  const canvas = owner.createElement("canvas");
  const canvasPane = owner.createElement("div");
  canvasPane.className = "pdf-page";
  canvasPane.tabIndex = 0;
  canvasPane.setAttribute("role", "region");
  canvasPane.setAttribute("aria-label", `PDF page: ${file.name}`);
  canvasPane.append(canvas);
  canvas.setAttribute("role", "img");
  canvas.setAttribute("aria-label", `PDF: ${file.name}`);
  const pageLabel = owner.createElement("label");
  pageLabel.textContent = "Page ";
  const pageNumber = owner.createElement("input");
  pageNumber.type = "number";
  pageNumber.min = "1";
  pageNumber.value = "1";
  pageNumber.setAttribute("aria-label", `PDF page: ${file.name}`);
  pageLabel.append(pageNumber);
  const zoomLabel = owner.createElement("label");
  zoomLabel.textContent = "Zoom ";
  const zoomSelect = owner.createElement("select");
  zoomSelect.setAttribute("aria-label", `PDF zoom: ${file.name}`);
  for (const [value, text] of [
    ["1", "Fit width"],
    ["0.75", "75%"],
    ["1.25", "125%"],
    ["1.5", "150%"],
    ["2", "200%"],
  ])
    zoomSelect.add(new Option(text, value));
  zoomLabel.append(zoomSelect);
  const previous = action("Previous page", () => {
    number -= 1;
    void render();
  });
  const next = action("Next page", () => {
    number += 1;
    void render();
  });
  toolbar.append(previous, pageLabel, next, zoomLabel);
  const textDetails = owner.createElement("details");
  const textSummary = owner.createElement("summary");
  textSummary.textContent = "Page text";
  const text = owner.createElement("div");
  text.className = "pdf-text";
  textDetails.append(textSummary, text);
  const links = owner.createElement("div");
  const passwordForm = owner.createElement("form");
  passwordForm.hidden = true;
  const passwordLabel = owner.createElement("label");
  passwordLabel.textContent = "PDF password ";
  const password = owner.createElement("input");
  password.type = "password";
  password.autocomplete = "off";
  passwordLabel.append(password);
  const unlock = owner.createElement("button");
  unlock.type = "submit";
  unlock.textContent = "Unlock PDF";
  passwordForm.append(passwordLabel, unlock);
  host.append(status, passwordForm, toolbar, canvasPane, textDetails, links);
  function controls(): void {
    previous.disabled = !document || number <= 1;
    next.disabled = !document || number >= document.numPages;
    pageNumber.disabled = !document;
    zoomSelect.disabled = !document;
    pageNumber.value = String(number);
    if (document) pageNumber.max = String(document.numPages);
  }
  async function render(): Promise<void> {
    if (!document || disposed) return;
    const token = ++generation;
    rendering?.cancel();
    controls();
    status.textContent = `Loading page ${number}`;
    try {
      const page = await document.getPage(number);
      if (disposed || token !== generation) return;
      const original = page.getViewport({ scale: 1 });
      const width = Math.max(100, canvasPane.clientWidth || 600);
      const scale = Math.min(width / original.width, 2) * zoom;
      const viewport = page.getViewport({ scale });
      const ratio = Math.min(
        window.devicePixelRatio || 1,
        2,
        Math.sqrt(8_000_000 / (viewport.width * viewport.height)),
      );
      canvas.width = Math.max(1, Math.floor(viewport.width * ratio));
      canvas.height = Math.max(1, Math.floor(viewport.height * ratio));
      canvas.style.width = `${Math.floor(viewport.width)}px`;
      canvas.style.height = `${Math.floor(viewport.height)}px`;
      const context = canvas.getContext("2d");
      if (!context) throw new Error("Canvas rendering is unavailable.");
      canvas.setAttribute("aria-label", `Page ${number} of ${document.numPages}: ${file.name}`);
      rendering = page.render({
        canvas,
        canvasContext: context,
        viewport,
        transform: [ratio, 0, 0, ratio, 0, 0],
      });
      await rendering.promise;
      const content = await page.getTextContent();
      const annotations: unknown = await page.getAnnotations();
      if (disposed || token !== generation) return;
      text.textContent = content.items
        .map((item) => ("str" in item ? `${item.str}${item.hasEOL ? "\n" : " "}` : ""))
        .join("");
      if (!text.textContent.trim()) text.textContent = "No extractable text on this page.";
      links.replaceChildren();
      if (Array.isArray(annotations))
        for (const raw of annotations) {
          const entry: unknown = raw;
          if (
            !record(entry) ||
            typeof entry.url !== "string" ||
            !/^(https?:|mailto:)/i.test(entry.url)
          )
            continue;
          const link = owner.createElement("a");
          link.href = entry.url;
          link.textContent = entry.url;
          link.target = "_blank";
          link.rel = "noopener noreferrer";
          link.className = "control";
          links.append(link);
        }
      status.textContent = `Page ${number} of ${document.numPages}`;
    } catch (error) {
      if (!disposed && token === generation)
        status.textContent =
          error instanceof Error
            ? `PDF page unavailable: ${error.message}`
            : "PDF page unavailable.";
    }
  }
  pageNumber.addEventListener("change", () => {
    if (
      document &&
      Number.isInteger(pageNumber.valueAsNumber) &&
      pageNumber.valueAsNumber >= 1 &&
      pageNumber.valueAsNumber <= document.numPages
    ) {
      number = pageNumber.valueAsNumber;
      void render();
    } else controls();
  });
  zoomSelect.addEventListener("change", () => {
    zoom = Number(zoomSelect.value);
    void render();
  });
  GlobalWorkerOptions.workerSrc = "/vendor/pdf/pdf.worker.mjs";
  const loading = getDocument({
    url: file.url,
    cMapUrl: "/vendor/pdf/cmaps/",
    cMapPacked: true,
    standardFontDataUrl: "/vendor/pdf/standard_fonts/",
    wasmUrl: "/vendor/pdf/wasm/",
    useWasm: false,
    enableXfa: false,
  });
  let updatePassword: ((value: string) => void) | null = null;
  loading.onPassword = (update: (value: string) => void, reason: number): void => {
    if (disposed) return;
    updatePassword = update;
    passwordForm.hidden = false;
    status.textContent = reason === 2 ? "Incorrect PDF password." : "Enter the PDF password.";
  };
  passwordForm.addEventListener("submit", (event) => {
    event.preventDefault();
    updatePassword?.(password.value);
    password.value = "";
  });
  void loading.promise
    .then(async (value) => {
      if (disposed) return;
      document = value;
      passwordForm.hidden = true;
      controls();
      await render();
    })
    .catch((error: unknown) => {
      if (!disposed)
        status.textContent =
          error instanceof Error ? `PDF unavailable: ${error.message}` : "PDF unavailable.";
    });
  controls();
  return () => {
    disposed = true;
    ++generation;
    rendering?.cancel();
    void loading.destroy().catch(() => {});
  };
}

export function mountFile(host: HTMLElement, file: Attachment): () => void {
  const caption = document.createElement("p");
  caption.textContent = file.name;
  host.append(caption);
  let dispose = (): void => {};
  if (file.kind === "image") {
    const image = document.createElement("img");
    image.src = file.url;
    image.alt = file.description || file.name;
    image.addEventListener("error", () => {
      caption.textContent = `Image unavailable: ${file.name}`;
    });
    host.append(image);
  } else if (file.kind === "audio" || file.kind === "video") {
    const media = document.createElement(file.kind);
    media.src = file.url;
    media.controls = true;
    media.preload = "metadata";
    media.setAttribute("aria-label", file.name);
    if (media instanceof HTMLVideoElement) media.playsInline = true;
    const error = document.createElement("p");
    error.hidden = true;
    error.setAttribute("role", "status");
    media.addEventListener("error", () => {
      error.textContent =
        "Playback is unavailable for this file in this browser; download the original file.";
      error.hidden = false;
    });
    host.append(media, error);
    dispose = () => {
      media.pause();
      media.removeAttribute("src");
      media.load();
    };
  } else if (file.kind === "pdf") {
    let active = true;
    let stop = (): void => {};
    void import("./pdf.mjs")
      .then((pdf) => {
        if (active) stop = pdfView(host, file, pdf);
      })
      .catch(() => {
        if (active) caption.textContent = `PDF unavailable: ${file.name}`;
      });
    dispose = () => {
      active = false;
      stop();
    };
  }
  if (file.description) {
    const description = document.createElement("p");
    description.textContent = file.description;
    description.className = "file-description";
    host.append(description);
  }
  const links = document.createElement("div");
  links.className = "actions";
  const open = document.createElement("a");
  open.href = file.url;
  open.target = "_blank";
  open.rel = "noopener noreferrer";
  open.className = "control";
  open.textContent = `Open ${file.name}`;
  links.append(open, download(file), fileOptions(file));
  host.append(links);
  return dispose;
}

export interface MediaPreviews {
  update(container: HTMLElement, source: string): void;
  clear(): void;
}

export function createMediaPreviews(api: (path: string) => Promise<unknown>): MediaPreviews {
  const files = new Map<string, { host: HTMLDivElement; dispose(): void }>();
  function clear(): void {
    for (const file of files.values()) file.dispose();
    files.clear();
  }
  function update(container: HTMLElement, source: string): void {
    const retained = new Set(
      Array.from(
        source.matchAll(/\.attachments\/([a-f0-9]{32}\.[a-zA-Z0-9]{1,12})/g),
        (match) => match[1],
      ),
    );
    const referenced = new Set<string>();
    const positions = new Map<Element, Element>();
    for (const image of container.querySelectorAll<HTMLImageElement>("img")) {
      if (image.closest("a")) continue;
      const link = document.createElement("a");
      link.href = image.src;
      link.target = "_blank";
      link.rel = "noopener noreferrer";
      link.setAttribute("aria-label", `Open ${image.alt || "image"}`);
      link.className = "image-link";
      image.replaceWith(link);
      link.append(image);
    }
    for (const link of container.querySelectorAll<HTMLAnchorElement>('a[href^="/.attachments/"]')) {
      if (link.closest(".file-preview")) continue;
      const id = /^\/\.attachments\/([a-f0-9]{32}\.[a-zA-Z0-9]{1,12})$/.exec(
        link.getAttribute("href") ?? "",
      )?.[1];
      if (!id || referenced.has(id)) continue;
      referenced.add(id);
      let file = files.get(id);
      if (!file) {
        const host = document.createElement("div");
        host.className = "file-preview";
        const entry = { host, dispose: (): void => {} };
        files.set(id, entry);
        file = entry;
        void api(`/api/attachments/${id}`)
          .then((value) => {
            if (files.get(id) !== entry) return;
            const file = parseAttachment(value);
            if (link.querySelector("img")) host.append(download(file), fileOptions(file));
            else entry.dispose = mountFile(host, file);
          })
          .catch(() => {
            if (files.get(id) === entry) host.textContent = "Attached file unavailable.";
          });
      }
      let block: Element = link.closest("p") ?? link;
      while (
        block.parentElement &&
        block.parentElement !== container &&
        !/^(LI|TD|TH|BLOCKQUOTE|ARTICLE|SECTION|DIV)$/.test(block.parentElement.tagName)
      )
        block = block.parentElement;
      (positions.get(block) ?? block).after(file.host);
      positions.set(block, file.host);
    }
    for (const [id, file] of files)
      if (!referenced.has(id) && !retained.has(id)) {
        file.dispose();
        file.host.remove();
        files.delete(id);
      }
  }
  return { update, clear };
}
