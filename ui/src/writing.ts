import { afterWritingLayout, writingCaret, writingElement } from "./surface.js";

export function paperHeight(body: HTMLTextAreaElement): number {
  if (!body.value) return 0;
  const style = getComputedStyle(body);
  const probe = document.createElement("textarea");
  probe.setAttribute("aria-hidden", "true");
  probe.tabIndex = -1;
  for (const property of [
    "font",
    "line-height",
    "letter-spacing",
    "word-spacing",
    "text-indent",
    "tab-size",
    "padding",
    "border",
    "box-sizing",
    "direction",
    "white-space",
    "overflow-wrap",
    "word-break",
  ])
    probe.style.setProperty(property, style.getPropertyValue(property));
  Object.assign(probe.style, {
    position: "fixed",
    left: "-10000px",
    top: "0",
    visibility: "hidden",
    width: `${body.getBoundingClientRect().width}px`,
    height: "0",
    minHeight: "0",
    maxHeight: "none",
    overflow: "hidden",
  });
  probe.value = body.value;
  document.body.append(probe);
  const height =
    probe.scrollHeight +
    Number.parseFloat(style.borderTopWidth) +
    Number.parseFloat(style.borderBottomWidth);
  probe.remove();
  return Math.ceil(height);
}

export function caretBox(body: HTMLTextAreaElement): DOMRect {
  const live = writingCaret();
  if (live) return live;
  const style = getComputedStyle(body);
  const mirror = document.createElement("div");
  mirror.setAttribute("aria-hidden", "true");
  for (const property of [
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "line-height",
    "letter-spacing",
    "word-spacing",
    "text-indent",
    "tab-size",
    "padding",
    "border",
    "box-sizing",
    "direction",
  ])
    mirror.style.setProperty(property, style.getPropertyValue(property));
  Object.assign(mirror.style, {
    position: "fixed",
    left: "-10000px",
    top: "0",
    visibility: "hidden",
    width: `${body.clientWidth}px`,
    whiteSpace: "pre-wrap",
    overflowWrap: "break-word",
  });
  mirror.textContent = body.value.slice(0, body.selectionEnd);
  const marker = document.createElement("span");
  marker.textContent = "\u200b";
  mirror.append(marker);
  document.body.append(mirror);
  const origin = mirror.getBoundingClientRect();
  const cursor = marker.getBoundingClientRect();
  const input = body.getBoundingClientRect();
  const result = new DOMRect(
    input.left + cursor.left - origin.left,
    input.top + cursor.top - origin.top - body.scrollTop,
    1,
    Number.parseFloat(style.lineHeight),
  );
  mirror.remove();
  return result;
}

export function initWriting(
  body: HTMLTextAreaElement,
  enabled: () => boolean,
  composing: () => boolean,
): void {
  const target = document.getElementById("typing-space");
  if (!target) throw new Error("Missing typing space");
  const spacer = target;
  let scheduled = false;
  function follow(): void {
    scheduled = false;
    if (!enabled() || !body.value) {
      spacer.style.height = "0px";
      return;
    }
    if (
      composing() ||
      !writingElement(body).contains(document.activeElement) ||
      !writingElement(body).checkVisibility() ||
      document.querySelector("dialog[open]")
    )
      return;
    const caret = caretBox(body);
    const center = (visualViewport?.offsetTop ?? 0) + (visualViewport?.height ?? innerHeight) / 2;
    if (caret.top + scrollY < center) return;
    const top = scrollY + caret.top + caret.height / 2 - center;
    spacer.style.height = `${Math.ceil(innerHeight - center + caret.height)}px`;
    window.scrollTo({ top, behavior: "instant" });
  }
  function schedule(): void {
    if (scheduled) return;
    scheduled = true;
    afterWritingLayout(follow);
  }
  body.addEventListener("input", schedule);
  body.addEventListener("compositionend", schedule);
  body.addEventListener("keyup", (event) => {
    if (["ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown"].includes(event.key))
      schedule();
  });
  document.addEventListener("tungsten:note", () => {
    spacer.style.height = "0px";
  });
  visualViewport?.addEventListener("resize", schedule);
}
