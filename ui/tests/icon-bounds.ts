export function clippedIcons(fixture: "clipped" | "padded" | null): string[] {
  const sample = document.querySelector<SVGSVGElement>("svg.control-icon");
  const inserted = fixture && sample ? (sample.cloneNode(true) as SVGSVGElement) : null;
  if (inserted) {
    inserted.dataset.icon = "bounds-fixture";
    inserted.setAttribute("viewBox", fixture === "clipped" ? "0 0 24 24" : "-1 -1 26 26");
    inserted.querySelector("path")?.setAttribute("d", "M0 12H24");
    document.body.append(inserted);
  }
  const findings: string[] = [];
  for (const svg of document.querySelectorAll<SVGSVGElement>("svg.control-icon")) {
    if (!svg.checkVisibility()) continue;
    const path = svg.querySelector("path");
    if (!path) {
      findings.push(svg.dataset.icon ?? "missing path");
      continue;
    }
    const box = path.getBBox();
    const view = svg.viewBox.baseVal;
    const padding = Number.parseFloat(getComputedStyle(svg).strokeWidth) / 2;
    if (
      box.x - padding < view.x ||
      box.y - padding < view.y ||
      box.x + box.width + padding > view.x + view.width ||
      box.y + box.height + padding > view.y + view.height
    )
      findings.push(svg.dataset.icon ?? "icon");
  }
  inserted?.remove();
  return findings;
}
