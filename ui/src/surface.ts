export interface WritingSurface {
  element: HTMLElement;
  visible(): boolean;
  caret(): DOMRect | null;
  focus(): void;
  select(from: number, to: number): void;
  afterLayout(callback: () => void): void;
}

let surface: WritingSurface | null = null;

export function registerSurface(value: WritingSurface | null): void {
  surface = value;
}

export function writingElement(body: HTMLTextAreaElement): HTMLElement {
  return surface?.visible() ? surface.element : body;
}

export function writingCaret(): DOMRect | null {
  return surface?.visible() ? surface.caret() : null;
}

export function afterWritingLayout(callback: () => void): void {
  if (surface?.visible()) surface.afterLayout(callback);
  else requestAnimationFrame(callback);
}

export function focusWriting(body: HTMLTextAreaElement): void {
  if (surface?.visible()) surface.focus();
  else body.focus();
}

export function observeLayout(targets: readonly Element[], update: () => void): () => void {
  let frame: number | null = null;
  const observer = new ResizeObserver(() => {
    if (frame !== null) return;
    frame = requestAnimationFrame(() => {
      frame = null;
      update();
    });
  });
  for (const target of targets) observer.observe(target);
  return () => {
    observer.disconnect();
    if (frame !== null) cancelAnimationFrame(frame);
    frame = null;
  };
}

export function selectWriting(body: HTMLTextAreaElement, from: number, to: number): void {
  body.setSelectionRange(from, to);
  if (surface?.visible()) surface.select(body.selectionStart, body.selectionEnd);
}
