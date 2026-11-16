export function initDialogChrome(): void {
  for (const dialog of document.querySelectorAll<HTMLDialogElement>("dialog")) {
    dialog.addEventListener("keydown", (event) => {
      if (event.key !== "Tab" || !dialog.matches(":modal")) return;
      const targets = [
        ...dialog.querySelectorAll<HTMLElement>(
          "button, a[href], input, textarea, select, [tabindex]",
        ),
      ].filter(
        (node) => node.tabIndex >= 0 && node.checkVisibility() && !node.matches(":disabled"),
      );
      const first = targets[0];
      const last = targets.at(-1);
      if (!first || !last) return;
      if (
        event.shiftKey &&
        (document.activeElement === first ||
          !targets.some((node) => node === document.activeElement))
      ) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    });
  }
  for (const close of document.querySelectorAll<HTMLButtonElement>("dialog [data-close]")) {
    close.setAttribute("aria-label", "Close");
    close.title = "Close";
    close.classList.add("dialog-close", "icon-button");
    const icon = document.createElement("span");
    icon.setAttribute("aria-hidden", "true");
    icon.textContent = "×";
    close.replaceChildren(icon);
  }
}

export function openDialog(
  dialog: HTMLDialogElement,
  opener: HTMLElement | null = null,
  modal = true,
): void {
  const trigger =
    opener ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
  const parent = trigger?.closest("dialog");
  if (parent instanceof HTMLDialogElement && parent !== dialog) parent.close();
  if (dialog.open) return;
  trigger?.focus({ preventScroll: true });
  if (modal) dialog.showModal();
  else dialog.show();
  dialog.addEventListener(
    "close",
    () => {
      if (document.querySelector("dialog[open]")) return;
      const active = document.activeElement;
      if (
        active instanceof HTMLElement &&
        active !== document.body &&
        !active.closest("dialog:not([open]), [popover]:not(:popover-open), [hidden]")
      )
        return;
      const target = trigger?.closest("dialog:not([open]), [popover]:not(:popover-open), [hidden]")
        ? document.getElementById("toggle-sidebar")
        : trigger;
      target?.focus({ preventScroll: true });
    },
    { once: true },
  );
}
