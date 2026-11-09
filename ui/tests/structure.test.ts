import assert from "node:assert/strict";
import { test } from "node:test";
import { pairedDeletion } from "../src/delimiters.ts";
import { normalizeHeadings } from "../src/headings.ts";

test("normalizes one title and continuous heading ranks without changing code", () => {
  const source =
    "### Title\n\n###### One\n\n# Two\n\n##### Three\n\n## Four\n\n```md\n# literal\n###### literal\n```\n";
  const expected =
    "# Title\n\n## One\n\n## Two\n\n### Three\n\n## Four\n\n```md\n# literal\n###### literal\n```\n";
  assert.equal(normalizeHeadings(source), expected);
  assert.equal(normalizeHeadings(expected), expected);
  assert.equal(normalizeHeadings(""), "");
  assert.equal(normalizeHeadings("Plain paragraph\n\nNo title"), "Plain paragraph\n\nNo title");
  assert.equal(normalizeHeadings("Title\n---\n\nSecond\n===\n"), "Title\n===\n\nSecond\n---\n");
  assert.equal(
    normalizeHeadings("# Title\n\n> # Quoted heading\n\n- ##### Nested heading"),
    "# Title\n\n> ## Quoted heading\n\n- ### Nested heading",
  );
});

test("deleting Markdown delimiters preserves their contents and removes their matching marks", () => {
  for (const marker of ["*", "**", "_", "__", "~~", "`", "``"]) {
    const text = `Before ${marker}日本語 😀${marker} after`;
    const from = 7;
    const edit = pairedDeletion({ text, start: from, end: from + marker.length }, "backward");
    assert.deepEqual(edit, { text: "Before 日本語 😀 after", start: from, end: from });
    const close = from + marker.length + "日本語 😀".length;
    assert.deepEqual(
      pairedDeletion({ text, start: close, end: close + marker.length }, "forward"),
      { text: "Before 日本語 😀 after", start: close - marker.length, end: close - marker.length },
    );
  }
  assert.deepEqual(pairedDeletion({ text: "**bold**", start: 2, end: 2 }, "backward"), {
    text: "*bold*",
    start: 1,
    end: 1,
  });
  assert.equal(pairedDeletion({ text: "a * unclosed", start: 3, end: 3 }, "backward"), null);
  assert.equal(pairedDeletion({ text: "\\*literal*", start: 2, end: 2 }, "backward"), null);
  assert.equal(
    pairedDeletion({ text: "```txt\n*literal*\n```", start: 8, end: 8 }, "backward"),
    null,
  );
  assert.deepEqual(pairedDeletion({ text: "[label](url)", start: 0, end: 1 }, "backward"), {
    text: "label(url)",
    start: 0,
    end: 0,
  });
});

test("deleting either fence unwraps its own block with language metadata and unequal fence lengths", () => {
  for (const marker of ["```", "~~~", "````"]) {
    const text = `# Title\n\n${marker}python extra\nimport package\n${marker}${marker[0]}\n\nTail`;
    assert.deepEqual(pairedDeletion({ text, start: 9, end: 9 + marker.length }, "backward"), {
      text: "# Title\n\nimport package\n\nTail",
      start: 9,
      end: 9,
    });
  }
  const text = "```js\nfirst()\n```\n\n```py\nsecond()\n```";
  assert.deepEqual(pairedDeletion({ text, start: 14, end: 17 }, "forward"), {
    text: "first()\n\n```py\nsecond()\n```",
    start: 8,
    end: 8,
  });
  assert.equal(pairedDeletion({ text: "```js\nnot closed", start: 3, end: 3 }, "backward"), null);
});
