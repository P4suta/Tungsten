import assert from "node:assert/strict";
import { test } from "node:test";
import { graphResponse, notePage } from "../src/catalog.ts";

test("bounded catalog responses reject malformed and excessive results", () => {
  const note = {
    name: "a.md",
    title: "A",
    snippet: "",
    modified: 1,
    bytes: 1,
    pinned: false,
    tags: [],
  };
  assert.equal(notePage({ notes: [note], next: "cursor", total: 1 }).notes[0]?.name, "a.md");
  assert.equal(
    graphResponse({ notes: [note], edges: [], total: 1000000, neighbors: 0, truncated: false })
      .total,
    1000000,
  );
  for (const value of [
    null,
    { notes: [], next: 1, total: 1 },
    { notes: [{ ...note, modified: NaN }], next: null, total: 1 },
    { notes: Array(201).fill(note), next: null, total: 201 },
  ])
    assert.throws(() => notePage(value));
  assert.throws(() =>
    graphResponse({
      notes: [note],
      edges: [{ source: 1, target: "a.md" }],
      total: 1,
      neighbors: 0,
      truncated: false,
    }),
  );
  assert.throws(() =>
    graphResponse({
      notes: Array(121).fill(note),
      edges: [],
      total: 121,
      neighbors: 120,
      truncated: false,
    }),
  );
});
