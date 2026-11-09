export function missingIcons(fixture: "missing" | "valid" | null): string[] {
  const button = fixture ? document.createElement("button") : null;
  if (button) {
    button.id = "missing-icon-fixture";
    button.textContent = "Fixture";
    const svg = document.querySelector("svg.control-icon");
    if (fixture === "valid" && svg) button.append(svg.cloneNode(true));
    document.body.append(button);
  }
  const findings = Array.from(
    document.querySelectorAll<HTMLElement>(
      "button, a[href], [role=link], summary, input, select, textarea, [role=option], [contenteditable=true]",
    ),
  )
    .filter((node) => {
      if (!node.checkVisibility() || !node.getBoundingClientRect().width) return false;
      if (node.matches("input[type=checkbox], input[type=radio]")) return false;
      if (node.matches("input, select, textarea"))
        return !node.parentElement?.querySelector("svg[aria-hidden=true]");
      if (node.matches("[contenteditable=true]")) return !node.closest("#live-editor");
      return !node.querySelector("svg[aria-hidden=true]");
    })
    .map((node) => node.id || node.getAttribute("aria-label") || node.textContent || node.tagName);
  button?.remove();
  return findings;
}
