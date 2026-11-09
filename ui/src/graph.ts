export interface NoteEdge {
  source: string;
  target: string;
}

export interface GraphSlice {
  names: string[];
  edges: NoteEdge[];
  total: number;
}

export function graphSlice(
  names: readonly string[],
  edges: readonly NoteEdge[],
  focus: string | null,
  limit = 120,
): GraphSlice {
  const known = new Set(names);
  const valid = edges.filter((edge) => known.has(edge.source) && known.has(edge.target));
  const degree = new Map<string, number>();
  for (const edge of valid) {
    degree.set(edge.source, (degree.get(edge.source) ?? 0) + 1);
    degree.set(edge.target, (degree.get(edge.target) ?? 0) + 1);
  }
  const selected = new Set<string>();
  const nearby = new Set<string>();
  if (focus && known.has(focus)) {
    selected.add(focus);
    for (const edge of valid) {
      if (edge.source === focus) nearby.add(edge.target);
      if (edge.target === focus) nearby.add(edge.source);
    }
    for (let depth = 0; depth < 2; depth++) {
      const neighbors = valid.flatMap((edge) =>
        selected.has(edge.source) ? [edge.target] : selected.has(edge.target) ? [edge.source] : [],
      );
      for (const name of neighbors) selected.add(name);
    }
  } else for (const name of names) selected.add(name);
  const visible = [...selected]
    .sort(
      (a, b) =>
        Number(b === focus) - Number(a === focus) ||
        Number(nearby.has(b)) - Number(nearby.has(a)) ||
        (degree.get(b) ?? 0) - (degree.get(a) ?? 0) ||
        a.localeCompare(b),
    )
    .slice(0, limit);
  const visibleSet = new Set(visible);
  return {
    names: visible,
    edges: valid.filter((edge) => visibleSet.has(edge.source) && visibleSet.has(edge.target)),
    total: selected.size,
  };
}
