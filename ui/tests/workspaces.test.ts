import assert from "node:assert/strict";
import { test } from "node:test";
import { validWorkspaceId, workspace, workspaceNotes, workspacePage } from "../src/workspaces.ts";

const revision = `"${"a".repeat(64)}"`;
const group = { id: "9223372036854775807", title: "Writing 😀", revision };

test("workspace contracts preserve SQLite identities beyond JavaScript integer precision", () => {
  assert.equal(validWorkspaceId(group.id), true);
  assert.deepEqual(workspace(group), group);
  for (const value of [1, null, "", "0", "01", "+1", "1.0", "-1", "9223372036854775808", "1e9"])
    assert.equal(validWorkspaceId(value), false);
  for (const value of [
    null,
    {},
    { ...group, title: " " },
    { ...group, title: "😀".repeat(121) },
    { ...group, revision: "a".repeat(64) },
  ])
    assert.throws(() => workspace(value));
});

test("workspace pages reject unbounded, duplicated, or malformed data", () => {
  const page = { workspaces: [group], next: "9", revision };
  assert.deepEqual(workspacePage(page), page);
  for (const value of [
    null,
    { ...page, next: 2 },
    { ...page, revision: null },
    { ...page, workspaces: [group, group] },
    { ...page, workspaces: Array(51).fill(group) },
  ])
    assert.throws(() => workspacePage(value));
  const notes = {
    workspace: group,
    notes: [{ name: "安定.md", title: "Thought" }],
    next: null,
    revision,
  };
  assert.deepEqual(workspaceNotes(notes), notes);
  for (const value of [
    null,
    { ...notes, next: "00" },
    { ...notes, notes: [{ name: "😀".repeat(61), title: "Large" }] },
    { ...notes, notes: [notes.notes[0], notes.notes[0]] },
    { ...notes, notes: Array(51).fill(notes.notes[0]) },
    { ...notes, workspace: {} },
  ])
    assert.throws(() => workspaceNotes(value));
});
