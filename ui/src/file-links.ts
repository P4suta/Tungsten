import { parser } from "@lezer/markdown";

interface LinkDestination {
  label: string;
  from: number;
  to: number;
  destination: string;
}

export interface ChangedFileLink {
  from: number;
  to: number;
  destination: string;
  previous: string;
  incomplete?: boolean;
  suffix?: string;
  repairable?: boolean;
}

function destinations(text: string): LinkDestination[] {
  const links: LinkDestination[] = [];
  parser.parse(text).iterate({
    enter(ref) {
      if (ref.name !== "Link" && ref.name !== "Image") return;
      const url = ref.node.getChild("URL");
      if (!url) return;
      links.push({
        label: text.slice(ref.from, url.from),
        from: url.from,
        to: url.to,
        destination: text.slice(url.from, url.to),
      });
    },
  });
  return links;
}

export function changedFileLinks(before: string, after: string): ChangedFileLink[] {
  const previous = destinations(before);
  const current = destinations(after);
  const changed: ChangedFileLink[] = current.flatMap((link) => {
    if (/^https?:\/\//i.test(link.destination)) return [];
    const candidates = previous.filter(
      (entry) => entry.label === link.label && entry.destination.includes(".attachments/"),
    );
    const original = candidates[0];
    return original && candidates.length === 1 && original.destination !== link.destination
      ? [
          {
            from: link.from,
            to: link.to,
            destination: link.destination,
            previous: original.destination,
          },
        ]
      : [];
  });
  for (const original of previous) {
    if (
      !original.destination.includes(".attachments/") ||
      previous.filter((entry) => entry.label === original.label).length !== 1 ||
      current.some((entry) => entry.label === original.label)
    )
      continue;
    const at = after.indexOf(original.label);
    if (at < 0 || after.indexOf(original.label, at + 1) >= 0) continue;
    let literal = false;
    for (let node = parser.parse(after).resolveInner(at, 1); node.parent; node = node.parent)
      if (["FencedCode", "CodeBlock", "InlineCode", "HTMLBlock"].includes(node.name))
        literal = true;
    if (literal) continue;
    const from = at + original.label.length;
    const newline = after.indexOf("\n", from);
    const end = newline < 0 ? after.length : newline;
    const close = after.indexOf(")", from);
    const complete = close >= 0 && close <= end;
    const to = complete ? close : end;
    const destination = after.slice(from, to);
    changed.push({
      from,
      to,
      destination,
      previous: original.destination,
      incomplete: true,
      suffix: complete ? "" : ")",
      repairable: complete || !/\s/.test(destination),
    });
  }
  return changed;
}

export function repairFileLinks(text: string, changes: ChangedFileLink[]): string {
  for (const change of [...changes].sort((a, b) => b.from - a.from))
    if (change.repairable !== false && text.slice(change.from, change.to) === change.destination)
      text =
        text.slice(0, change.from) +
        change.previous +
        (change.suffix ?? "") +
        text.slice(change.to);
  return text;
}

export function removeFileLinks(text: string, id: string): string {
  const ranges: { from: number; to: number }[] = [];
  parser.parse(text).iterate({
    enter(ref) {
      if (ref.name !== "Link" && ref.name !== "Image") return;
      const url = ref.node.getChild("URL");
      if (
        url &&
        /^(?:\.\.\/|\.\/|\/)*\.attachments\//.test(text.slice(url.from, url.to)) &&
        text.slice(url.from, url.to).endsWith(`.attachments/${id}`)
      )
        ranges.push({ from: ref.from, to: ref.to });
    },
  });
  for (const range of ranges.sort((a, b) => b.from - a.from))
    text = text.slice(0, range.from) + text.slice(range.to);
  return text;
}
