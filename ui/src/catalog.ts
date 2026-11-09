import type { NoteEdge } from "./graph.js";

export interface Summary {
  name: string;
  modified: number;
  pinned: boolean;
  tags: string[];
  snippet: string;
  bytes: number;
  title: string;
}

export interface NotePage {
  notes: Summary[];
  next: string | null;
  total: number;
}

export interface GraphResponse {
  notes: Summary[];
  edges: NoteEdge[];
  total: number;
  neighbors: number;
  truncated: boolean;
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function count(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

export function summaries(value: unknown): Summary[] {
  if (!Array.isArray(value) || value.length > 200) throw new Error("The note list is invalid.");
  return value.map((entry: unknown) => {
    if (
      !record(entry) ||
      typeof entry.name !== "string" ||
      !count(entry.modified) ||
      typeof entry.pinned !== "boolean" ||
      !Array.isArray(entry.tags) ||
      !entry.tags.every((tag: unknown) => typeof tag === "string") ||
      typeof entry.snippet !== "string" ||
      !count(entry.bytes) ||
      typeof entry.title !== "string"
    )
      throw new Error("The note list is invalid.");
    return {
      name: entry.name,
      modified: entry.modified,
      pinned: entry.pinned,
      tags: entry.tags,
      snippet: entry.snippet,
      bytes: entry.bytes,
      title: entry.title,
    };
  });
}

export function notePage(value: unknown): NotePage {
  if (Array.isArray(value)) return { notes: summaries(value), next: null, total: value.length };
  if (
    !record(value) ||
    !count(value.total) ||
    (value.next !== null && typeof value.next !== "string")
  )
    throw new Error("The search results are invalid.");
  return { notes: summaries(value.notes), next: value.next, total: value.total };
}

export function graphResponse(value: unknown): GraphResponse {
  if (
    !record(value) ||
    !Array.isArray(value.edges) ||
    !count(value.total) ||
    !count(value.neighbors) ||
    typeof value.truncated !== "boolean"
  )
    throw new Error("The note graph is invalid.");
  const notes = summaries(value.notes);
  const names = new Set(notes.map((note) => note.name));
  if (notes.length > 120 || names.size !== notes.length || value.edges.length > 2048)
    throw new Error("The note graph exceeds its display limit.");
  const edges = value.edges.map((edge: unknown): NoteEdge => {
    if (
      !record(edge) ||
      typeof edge.source !== "string" ||
      typeof edge.target !== "string" ||
      !names.has(edge.source) ||
      !names.has(edge.target)
    )
      throw new Error("The note graph is invalid.");
    return { source: edge.source, target: edge.target };
  });
  return {
    notes,
    edges,
    total: value.total,
    neighbors: value.neighbors,
    truncated: value.truncated,
  };
}
