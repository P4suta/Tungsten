import assert from "node:assert/strict";
import { test } from "node:test";
import { graphSlice } from "../src/graph.ts";
import { defaultTitle } from "../src/titles.ts";

test("graph neighborhoods follow real links in either direction and exclude missing targets", () => {
  const names = ["a", "b", "c", "d", "orphan"];
  const edges = [
    { source: "a", target: "b" },
    { source: "c", target: "b" },
    { source: "c", target: "d" },
    { source: "a", target: "missing" },
  ];
  const graph = graphSlice(names, edges, "a");
  assert.deepEqual(new Set(graph.names), new Set(["a", "b", "c"]));
  assert.equal(graph.names[0], "a");
  assert.equal(graph.edges.length, 2);
  assert.equal(graphSlice(names, edges, null).names.length, 5);
  assert.deepEqual(graphSlice(names, edges, "orphan").names, ["orphan"]);
  assert.equal(
    graphSlice(
      Array.from({ length: 200 }, (_, i) => String(i)),
      [],
      null,
    ).names.length,
    120,
  );
});

test("default titles use editable local timestamps with millisecond precision", () => {
  assert.equal(defaultTitle(new Date(2026, 10, 9, 1, 2, 3, 4)), "2026-11-09 01:02:03.004");
});

test("bounded neighborhoods keep explicit nearby links before popular distant nodes", () => {
  const edges = [
    { source: "topic", target: "nearby" },
    { source: "nearby", target: "popular" },
    ...Array.from({ length: 20 }, (_, index) => ({ source: `other-${index}`, target: "popular" })),
  ];
  const names = [...new Set(edges.flatMap((edge) => [edge.source, edge.target]))];
  assert.deepEqual(graphSlice(names, edges, "topic", 2).names, ["topic", "nearby"]);
});
