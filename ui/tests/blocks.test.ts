import assert from "node:assert/strict";
import { test } from "node:test";
import { completeInput, continueLine } from "../src/input.ts";
import { textLayout } from "../src/live-ranges.ts";

test("empty containers exit and quoted list tasks continue without touching literal code", () => {
  for (const prefix of ["-", "+", "*", "- ", "+ ", "* ", "1.", "2) ", "- [ ] ", ">", "> "]) {
    const text = `# Title\n\n${prefix}`;
    assert.deepEqual(continueLine({ text, start: text.length, end: text.length }), {
      text: "# Title\n\n",
      start: 9,
      end: 9,
    });
  }
  for (const [source, expected] of [
    ["> - [x] item", "> - [x] item\n> - [ ] "],
    ["> - ", "> "],
    ["> > ", "> "],
    ["```md\n- literal", null],
    ["~~~\n> literal", null],
    ["    - literal", "    - literal\n    "],
  ] as const) {
    const edit = continueLine({ text: source, start: source.length, end: source.length });
    if (expected === null) assert.equal(edit, null);
    else assert.deepEqual(edit, { text: expected, start: expected.length, end: expected.length });
  }
});

test("block rendering recognizes every bullet marker, nested quotes, and code fences", () => {
  const text = "- one\n+ two\n* three\n\n> quote\n\n```js\n*literal*\n```\n\nEnd";
  const layout = textLayout(text, { from: text.length, to: text.length });
  assert.equal(layout.marks.length, 0);
  assert.ok(layout.prefixes.some((range) => text.slice(range.from, range.to) === "```"));
  for (const marker of ["-", "+", "*", ">"])
    assert.ok(
      layout.prefixes.some((range) => text.slice(range.from, range.to) === marker),
      marker,
    );
});

test("container prefixes preserve marker widths and reveal only selected syntax", () => {
  for (const marker of ["-", "+", "*", "7.", "8)"]) {
    const text = `# Title\n\n${marker} item\n\nEnd`;
    const outside = textLayout(text, { from: text.length, to: text.length });
    assert.deepEqual(outside.prefixes, [
      {
        from: 9,
        to: 9 + marker.length,
        text: marker.length === 1 ? "•" : marker.replace(")", "."),
      },
    ]);
    assert.deepEqual(outside.lines, [{ from: 9, kind: "list" }]);
    for (let position = 9; position <= 9 + marker.length; position++)
      assert.deepEqual(textLayout(text, { from: position, to: position }).prefixes, [
        { from: 9, to: 9 + marker.length, text: null },
      ]);
    assert.deepEqual(textLayout(text, { from: 0, to: 8 }).prefixes, outside.prefixes);
    assert.deepEqual(
      textLayout(text, { from: 9 + marker.length + 1, to: text.length }).prefixes,
      outside.prefixes,
    );
  }
  const code = "```ts\ncode\n```\n\nEnd";
  assert.deepEqual(textLayout(code, { from: code.length, to: code.length }).lines, [
    { from: 0, kind: "code" },
    { from: 6, kind: "code" },
    { from: 11, kind: "code" },
    { from: 0, kind: "fence" },
    { from: 11, kind: "fence" },
  ]);
  const quote = "> first\n> > nested\n> last";
  assert.deepEqual(textLayout(quote, { from: quote.length, to: quote.length }).lines, [
    { from: 0, kind: "quote" },
    { from: 8, kind: "quote" },
    { from: 19, kind: "quote" },
  ]);
  assert.deepEqual(textLayout("    code\n    more", { from: 0, to: 0 }).lines, [
    { from: 0, kind: "code" },
    { from: 9, kind: "code" },
  ]);
  const emptyFence = textLayout("~~~", { from: 3, to: 3 });
  assert.deepEqual(emptyFence.lines, [
    { from: 0, kind: "code" },
    { from: 0, kind: "fence" },
  ]);
  assert.deepEqual(emptyFence.prefixes, [{ from: 0, to: 3, text: null }]);
});

test("literal blocks keep Markdown punctuation while containers retain their own prefix", () => {
  for (const source of ["```md\n", "~~~\n", "    code "])
    for (const token of ["*", "_", "`"])
      assert.equal(
        completeInput({ text: source, start: source.length, end: source.length }, token),
        null,
      );
  for (const [source, expected] of [
    ["+ item", "+ item\n+ "],
    ["* item", "* item\n* "],
    ["> 1. item", "> 1. item\n> 2. "],
    ["  > - ", "  > "],
    ["> >", "> "],
    ["- word > text", "- word > text\n- "],
    ["> \tword", "> \tword\n> "],
    ["\tcode", "\tcode\n\t"],
  ] as const)
    assert.deepEqual(continueLine({ text: source, start: source.length, end: source.length }), {
      text: expected,
      start: expected.length,
      end: expected.length,
    });
});

test("container continuation preserves list spacing and clears checked task state", () => {
  for (const [source, expected] of [
    ["-   item", "-   item\n-   "],
    ["- [x]   item", "- [x]   item\n- [ ] "],
    ["- \u00a0", ""],
  ] as const)
    assert.deepEqual(continueLine({ text: source, start: source.length, end: source.length }), {
      text: expected,
      start: expected.length,
      end: expected.length,
    });
});

test("fence decorations exclude literal contents and inline code", () => {
  const text = "```ts\ncode\n```\n\nEnd";
  assert.deepEqual(textLayout(text, { from: text.length, to: text.length }).prefixes, [
    { from: 0, to: 3, text: "" },
    { from: 11, to: 14, text: "" },
  ]);
  const mixed = textLayout("- x\n\n2) y\n\nEnd", { from: 14, to: 14 });
  assert.deepEqual(mixed.prefixes, [
    { from: 0, to: 1, text: "•" },
    { from: 5, to: 7, text: "2." },
  ]);
  const inline = textLayout("`code` end", { from: 10, to: 10 });
  assert.deepEqual(inline.prefixes, []);
  assert.deepEqual(inline.lines, []);
});
