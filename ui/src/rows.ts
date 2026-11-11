export function reconcileRows<T>(
  list: HTMLUListElement,
  entries: readonly T[],
  key: (entry: T) => string,
  render: (entry: T, row: HTMLLIElement | null) => HTMLLIElement,
): void {
  const existing = new Map<string, HTMLLIElement>();
  for (const node of [...list.childNodes])
    if (!(node instanceof HTMLLIElement) || !node.dataset.key) node.remove();
  for (const node of list.children) {
    if (node instanceof HTMLLIElement && node.dataset.key) existing.set(node.dataset.key, node);
  }
  let cursor = list.firstElementChild;
  for (const entry of entries) {
    const id = key(entry);
    const row = render(entry, existing.get(id) ?? null);
    row.dataset.key = id;
    existing.delete(id);
    if (row === cursor) cursor = cursor.nextElementSibling;
    else list.insertBefore(row, cursor);
  }
  for (const row of existing.values()) row.remove();
}
