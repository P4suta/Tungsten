import type { ForceCollide, Simulation, SimulationNodeDatum } from "d3-force";
import { graphResponse } from "./catalog.js";
import type { Snapshot, Summary } from "./features.js";
import { graphSlice, type NoteEdge } from "./graph.js";
import { icon } from "./icons.js";
import { noteLink } from "./links.js";
import { observeLayout, selectWriting } from "./surface.js";
import { defaultTitle, noteTitle } from "./titles.js";

interface HomePort {
  api(path: string): Promise<unknown>;
  state(): Snapshot | null;
  navigate(name: string): Promise<void>;
  blank(title: string, body: string): void;
}

interface GraphNode extends SimulationNodeDatum {
  name: string;
  x: number;
  y: number;
}

export interface HomeController {
  open(): Promise<void>;
  search(notes: Summary[]): void;
}

function element<T extends HTMLElement>(id: string, kind: { new (): T }): T {
  const node = document.getElementById(id);
  if (!(node instanceof kind)) throw new Error(`Missing graph control: ${id}`);
  return node;
}

export function initHome(port: HomePort): HomeController {
  const dialog = element("notes-dialog", HTMLDialogElement);
  const region = element("home-graph", HTMLElement);
  const layer = element("graph-nodes", HTMLDivElement);
  const svg = document.querySelector<SVGSVGElement>("#graph-edges");
  if (!svg) throw new Error("Missing graph edges");
  const lines = svg;
  const selection = element("graph-selection", HTMLDivElement);
  const status = element("graph-status", HTMLParagraphElement);
  const search = element("search", HTMLInputElement);
  let notes: Summary[] = [];
  let links: NoteEdge[] = [];
  let nodes: GraphNode[] = [];
  let visibleLinks: NoteEdge[] = [];
  let focus: string | null = null;
  let requestedFocus: string | null = null;
  let scale = 1;
  let panX = 0;
  let panY = 0;
  let generation = 0;
  let searchFocus: string | null = null;
  let capacity = 0;
  let collectionSize = 0;
  const visibleCapacity = (): number =>
    Math.min(
      120,
      Math.max(8, Math.floor(Math.min(region.clientWidth, region.clientHeight) / 96) ** 2),
    );
  const positions = new Map<string, { x: number; y: number }>();
  function rememberPositions(): void {
    for (const node of nodes) {
      positions.delete(node.name);
      positions.set(node.name, { x: node.x, y: node.y });
    }
    while (positions.size > 600) {
      const oldest = positions.keys().next().value;
      if (oldest === undefined) break;
      positions.delete(oldest);
    }
  }
  const buttons = new Map<string, HTMLButtonElement>();
  let simulation: Simulation<GraphNode, undefined> | null = null;
  let collisionRadius = 110;
  function separateTargets(): void {
    const radius = Math.max(110, 40 / scale);
    if (!simulation || radius === collisionRadius) return;
    collisionRadius = radius;
    simulation.force<ForceCollide<GraphNode>>("collision")?.radius(radius);
    simulation.tick(20);
  }
  let motionTimer: number | undefined;
  let frame: number | undefined;
  let edges: { line: SVGLineElement; source: GraphNode; target: GraphNode }[] = [];
  const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
  function stopMotion(): void {
    simulation?.stop();
    window.clearTimeout(motionTimer);
    region.dataset.motion = "idle";
  }
  function requestDraw(): void {
    if (frame !== undefined) return;
    frame = requestAnimationFrame(() => {
      frame = undefined;
      if (dialog.open) draw();
    });
  }
  function reheat(): void {
    if (reducedMotion.matches || document.hidden || !dialog.open) return;
    simulation?.alpha(0.25).restart();
    region.dataset.motion = "running";
    window.clearTimeout(motionTimer);
    motionTimer = window.setTimeout(stopMotion, 1800);
  }
  const namespace = "http://www.w3.org/2000/svg";
  let dragged = false;
  let pointer: {
    id: number;
    x: number;
    y: number;
    startX: number;
    startY: number;
    node: GraphNode | null;
    capture: HTMLElement;
  } | null = null;

  function draw(): void {
    const width = region.clientWidth;
    const height = region.clientHeight;
    const point = (node: GraphNode): { x: number; y: number } => ({
      x: width / 2 + panX + node.x * scale,
      y: height / 2 + panY + node.y * scale,
    });
    region.dataset.scale = String(scale);
    region.dataset.pan = `${panX},${panY}`;
    for (const node of nodes) {
      const button = buttons.get(node.name);
      if (!button) continue;
      const p = point(node);
      button.style.left = `${p.x}px`;
      button.style.top = `${p.y}px`;
    }
    const bounds = region.getBoundingClientRect();
    const targets = [...buttons.values()].map((button) => ({
      button,
      bounds: button.getBoundingClientRect(),
      label: button.querySelector("span")?.getBoundingClientRect(),
    }));
    const labels: DOMRect[] = [];
    const intersects = (a: DOMRect, b: DOMRect): boolean =>
      a.left < b.right + 4 && a.right + 4 > b.left && a.top < b.bottom + 4 && a.bottom + 4 > b.top;
    const visibility = targets.map((target) => {
      const box = target.label;
      if (!box) return false;
      const shown =
        box.left >= bounds.left &&
        box.right <= bounds.right &&
        box.top >= bounds.top &&
        box.bottom <= bounds.bottom &&
        !labels.some((other) => intersects(box, other)) &&
        !targets.some((other) => other.button !== target.button && intersects(box, other.bounds));
      if (shown) labels.push(box);
      return shown;
    });
    for (const [index, target] of targets.entries()) {
      target.button.classList.toggle("graph-labelled", visibility[index]);
    }
    for (const { line, source, target } of edges) {
      const a = point(source),
        b = point(target);
      const dx = b.x - a.x,
        dy = b.y - a.y;
      const button = buttons.get(target.name);
      const cut = Math.min(
        ((button?.offsetWidth ?? 120) / 2 + 5) / Math.max(1, Math.abs(dx)),
        ((button?.offsetHeight ?? 48) / 2 + 5) / Math.max(1, Math.abs(dy)),
        0.45,
      );
      line.setAttribute("x1", String(a.x));
      line.setAttribute("y1", String(a.y));
      line.setAttribute("x2", String(b.x - dx * cut));
      line.setAttribute("y2", String(b.y - dy * cut));
    }
    rememberPositions();
  }

  async function build(): Promise<void> {
    const { forceCenter, forceCollide, forceLink, forceManyBody, forceSimulation } = await import(
      "d3-force"
    );
    if (!dialog.open) return;
    stopMotion();
    capacity = visibleCapacity();
    const slice = graphSlice(
      notes.map((note) => note.name),
      links,
      focus,
      capacity,
    );
    nodes = slice.names.map((name) => ({
      name,
      x: positions.get(name)?.x ?? Number.NaN,
      y: positions.get(name)?.y ?? Number.NaN,
      fx: name === focus ? 0 : null,
      fy: name === focus ? 0 : null,
    }));
    const forceEdges = slice.edges.map((edge) => ({ ...edge }));
    collisionRadius = 110;
    simulation = forceSimulation(nodes)
      .force(
        "links",
        forceLink<GraphNode, { source: string | GraphNode; target: string | GraphNode }>(forceEdges)
          .id((node) => node.name)
          .distance(250),
      )
      .force("charge", forceManyBody().strength(-900))
      .force("collision", forceCollide<GraphNode>(110).iterations(3))
      .force("center", focus ? null : forceCenter(0, 0))
      .alphaDecay(0.07)
      .on("tick", requestDraw)
      .on("end", stopMotion)
      .stop();
    simulation.tick(40);
    rememberPositions();
    visibleLinks = slice.edges;
    lines.querySelector("g")?.remove();
    const group = document.createElementNS(namespace, "g");
    const byName = new Map(nodes.map((node) => [node.name, node]));
    edges = visibleLinks.flatMap((edge) => {
      const source = byName.get(edge.source),
        target = byName.get(edge.target);
      if (!source || !target || source === target) return [];
      const line = document.createElementNS(namespace, "line");
      line.setAttribute("marker-end", "url(#home-arrow)");
      group.append(line);
      return [{ line, source, target }];
    });
    lines.append(group);
    layer.replaceChildren();
    buttons.clear();
    for (const node of nodes) {
      const note = notes.find((note) => note.name === node.name);
      const button = document.createElement("button");
      button.type = "button";
      button.dataset.name = node.name;
      button.className = "graph-node";
      button.setAttribute("aria-pressed", String(node.name === focus));
      button.setAttribute("aria-label", `Explore ${note?.title || noteTitle(note?.snippet ?? "")}`);
      const label = document.createElement("span");
      label.textContent = note?.title || noteTitle(note?.snippet ?? "");
      button.title = label.textContent;
      button.append(icon("note"), label);
      button.addEventListener("click", (event) => {
        if (event.detail > 0 && dragged) return;
        void load(node.name).then(() => buttons.get(node.name)?.focus({ preventScroll: true }));
      });
      buttons.set(node.name, button);
      layer.append(button);
    }
    selection.replaceChildren();
    const selected = notes.find((note) => note.name === focus);
    if (selected) {
      const heading = document.createElement("h3");
      heading.textContent = selected.title || noteTitle(selected.snippet);
      const open = document.createElement("a");
      open.href = `/?note=${encodeURIComponent(selected.name)}`;
      open.className = "control";
      open.append(icon("note"), document.createTextNode("Open note"));
      open.addEventListener("click", (event) => {
        if (event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
        event.preventDefault();
        void port.navigate(selected.name);
      });
      selection.append(heading, open);
    }
    status.textContent =
      collectionSize > slice.names.length
        ? `${slice.names.length} of ${collectionSize} notes. Search to reach another note.`
        : `${slice.names.length} ${slice.names.length === 1 ? "note" : "notes"}`;
    fit();
    reheat();
  }

  function zoom(factor: number, x = region.clientWidth / 2, y = region.clientHeight / 2): void {
    const next = Math.max(0.01, Math.min(2.5, scale * factor));
    panX = ((panX + region.clientWidth / 2 - x) * next) / scale + x - region.clientWidth / 2;
    panY = ((panY + region.clientHeight / 2 - y) * next) / scale + y - region.clientHeight / 2;
    scale = next;
    separateTargets();
    draw();
  }
  element("graph-zoom-in", HTMLButtonElement).addEventListener("click", () => zoom(1.25));
  element("graph-zoom-out", HTMLButtonElement).addEventListener("click", () => zoom(0.8));
  function fit(): void {
    panX = 0;
    panY = 0;
    scale = Math.max(
      0.01,
      Math.min(
        1,
        ...nodes.flatMap((node) => {
          const button = buttons.get(node.name);
          return [
            (region.clientWidth - (button?.offsetWidth ?? 180) - 16) /
              (Math.max(1, Math.abs(node.x)) * 2),
            (region.clientHeight - (button?.offsetHeight ?? 48) - 16) /
              (Math.max(1, Math.abs(node.y)) * 2),
          ];
        }),
      ),
    );
    separateTargets();
    draw();
  }
  element("graph-fit", HTMLButtonElement).addEventListener("click", () => {
    stopMotion();
    fit();
  });
  element("graph-overview", HTMLButtonElement).addEventListener("click", () => {
    void load(null);
  });
  element("graph-create", HTMLButtonElement).addEventListener("click", () => {
    const title = defaultTitle(new Date());
    const selected = notes.find((note) => note.name === focus);
    dialog.close();
    port.blank(
      title,
      `# ${title}\n\n${selected ? `\n\n${noteLink("note.md", selected.name, selected.title || noteTitle(selected.snippet))}\n` : ""}`,
    );
    const body = document.querySelector<HTMLTextAreaElement>("#body");
    if (body) selectWriting(body, title.length + 4, title.length + 4);
  });
  region.addEventListener(
    "wheel",
    (event) => {
      event.preventDefault();
      const box = region.getBoundingClientRect();
      zoom(Math.exp(-event.deltaY / 300), event.clientX - box.left, event.clientY - box.top);
    },
    { passive: false },
  );
  region.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    const target =
      event.target instanceof Element
        ? event.target.closest<HTMLButtonElement>("button[data-name]")
        : null;
    pointer = {
      id: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      startX: event.clientX,
      startY: event.clientY,
      node: nodes.find((node) => node.name === target?.dataset.name) ?? null,
      capture: target ?? region,
    };
    dragged = false;
    pointer.capture.setPointerCapture(event.pointerId);
    if (pointer.node) {
      pointer.node.fx = pointer.node.x;
      pointer.node.fy = pointer.node.y;
      reheat();
    }
  });
  region.addEventListener("pointermove", (event) => {
    if (!pointer || pointer.id !== event.pointerId) return;
    const dx = event.clientX - pointer.x,
      dy = event.clientY - pointer.y;
    if (Math.hypot(event.clientX - pointer.startX, event.clientY - pointer.startY) > 5)
      dragged = true;
    if (pointer.node) {
      pointer.node.x += dx / scale;
      pointer.node.y += dy / scale;
      pointer.node.fx = pointer.node.x;
      pointer.node.fy = pointer.node.y;
      positions.set(pointer.node.name, { x: pointer.node.x, y: pointer.node.y });
      reheat();
    } else {
      panX += dx;
      panY += dy;
    }
    pointer.x = event.clientX;
    pointer.y = event.clientY;
    requestDraw();
  });
  const release = (event: PointerEvent): void => {
    if (pointer?.id === event.pointerId) {
      const capture = pointer.capture;
      if (pointer.node && pointer.node.name !== focus) {
        pointer.node.fx = null;
        pointer.node.fy = null;
      }
      pointer = null;
      if (capture.hasPointerCapture(event.pointerId))
        capture.releasePointerCapture(event.pointerId);
    }
  };
  region.addEventListener("pointerup", release);
  region.addEventListener("pointercancel", release);
  region.addEventListener("keydown", (event) => {
    if (event.ctrlKey || event.metaKey || event.altKey || event.isComposing) return;
    if (["+", "="].includes(event.key)) {
      event.preventDefault();
      zoom(1.25);
    } else if (event.key === "-") {
      event.preventDefault();
      zoom(0.8);
    } else if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) {
      event.preventDefault();
      const dx = event.key === "ArrowLeft" ? 30 : event.key === "ArrowRight" ? -30 : 0,
        dy = event.key === "ArrowUp" ? 30 : event.key === "ArrowDown" ? -30 : 0;
      const button = event.target instanceof HTMLButtonElement ? event.target : null;
      const target = button ? nodes.find((node) => node.name === button.dataset.name) : null;
      if (event.shiftKey && target) {
        target.x -= dx / scale;
        target.y -= dy / scale;
        target.fx = target.x;
        target.fy = target.y;
        positions.set(target.name, { x: target.x, y: target.y });
      } else {
        panX += dx;
        panY += dy;
      }
      draw();
    }
  });
  observeLayout([region], () => {
    if (!dialog.open) return;
    if (notes.length && capacity !== visibleCapacity()) {
      void load(requestedFocus);
    } else draw();
  });
  dialog.addEventListener("close", () => {
    ++generation;
    stopMotion();
    if (frame !== undefined) cancelAnimationFrame(frame);
    frame = undefined;
    pointer = null;
  });
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) stopMotion();
  });
  reducedMotion.addEventListener("change", () => {
    if (reducedMotion.matches) stopMotion();
  });
  dialog.addEventListener("keydown", (event) => {
    if (!event.isComposing && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "n") {
      event.preventDefault();
      element("graph-create", HTMLButtonElement).click();
    }
  });
  async function load(name: string | null): Promise<void> {
    requestedFocus = name;
    const current = ++generation;
    status.textContent = "Loading connections";
    try {
      const value = await port.api(
        `/api/graph?limit=${visibleCapacity()}${name ? `&name=${encodeURIComponent(name)}` : ""}`,
      );
      if (current !== generation || !dialog.open) return;
      const graph = graphResponse(value);
      notes = graph.notes;
      links = graph.edges;
      collectionSize = graph.total;
      focus = name && notes.some((note) => note.name === name) ? name : null;
      panX = 0;
      panY = 0;
      scale = 1;
      await build();
    } catch (error) {
      if (current === generation)
        status.textContent = error instanceof Error ? error.message : "The graph is unavailable.";
    }
  }
  return {
    async open() {
      const state = port.state();
      const name =
        state && !state.busy
          ? state.name
          : (new URL(location.href).searchParams.get("note") ?? state?.name ?? null);
      await load(search.value.trim() && searchFocus ? searchFocus : name);
    },
    search(matches) {
      searchFocus = matches[0]?.name ?? null;
      if (dialog.open && search.value.trim() && matches[0]) {
        void load(matches[0].name);
      }
    },
  };
}
