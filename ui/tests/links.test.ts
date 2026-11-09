import assert from "node:assert/strict";
import { test } from "node:test";
import { noteLink, noteLinkRange } from "../src/links.ts";

test("suggests note links only inside an unfinished Markdown label", () => {
  for (const text of ["[]()", "[Related]()", "[related", "before [Related]() after"]) {
    const start = text.includes("]") ? text.indexOf("]") : text.length;
    assert.deepEqual(noteLinkRange(text, start, start), {
      start: text.indexOf("["),
      end: text.includes("]()") ? text.indexOf("]()") + 3 : text.length,
      query: text.slice(text.indexOf("[") + 1, start).toLowerCase(),
    });
  }
  for (const text of [
    "plain",
    "\\[related",
    "![image]()",
    "`[code]()`",
    "```\n[code]",
    "    [code]",
    "[title](https://example.com)",
    "[one\ntwo]()",
    "[a] tail",
  ]) {
    const start = text.indexOf("]") < 0 ? text.length : text.indexOf("]");
    assert.equal(noteLinkRange(text, start, start), null, text);
  }
  assert.equal(noteLinkRange("[selected]()", 1, 4), null);
  assert.deepEqual(noteLinkRange("[one]()", 2, 2), { start: 0, end: 7, query: "o" });
  assert.deepEqual(noteLinkRange("\\\\[related", 10, 10), { start: 2, end: 10, query: "related" });
});

test("note links preserve stable relative paths and escape visible Markdown labels", () => {
  assert.equal(
    noteLink("source.md", "identity.md", "Visible title"),
    "[Visible title](identity.md)",
  );
  assert.equal(noteLink("a/source.md", "a/target.md", "Title"), "[Title](target.md)");
  assert.equal(noteLink("a/b/source.md", "a/c/target.md", "Title"), "[Title](../c/target.md)");
  assert.equal(noteLink("source.md", "a note.md", "[Title]"), "[\\[Title\\]](a%20note.md)");
});
