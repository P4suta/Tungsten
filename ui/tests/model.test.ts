import assert from "node:assert/strict";
import { test } from "node:test";
import { formatMarkdown, mappedCursor } from "../dist/formatting.js";
import {
  hasRemoteConflict,
  isSaved,
  mayApplyRemote,
  mayReadRemote,
  mayRecoverDraft,
  maySave,
} from "../dist/model.js";
import { matchingCommands, slashRange } from "../dist/slash.js";
import { noteTitle } from "../dist/titles.js";
import {
  completeInput,
  continueLine,
  indentLines,
  removePair,
  wrapSelection,
} from "../src/input.ts";

test("only dirty idle unblocked input can be sent", () => {
  const accepted: boolean[][] = [];
  for (const dirty of [false, true]) {
    for (const busy of [false, true]) {
      for (const composing of [false, true]) {
        for (const blocked of [false, true]) {
          if (maySave(dirty, busy, composing, blocked))
            accepted.push([dirty, busy, composing, blocked]);
        }
      }
    }
  }
  assert.deepEqual(accepted, [[true, false, false, false]]);
});

test("Saved requires acknowledged unchanged text and an idle writer", () => {
  const accepted: boolean[][] = [];
  for (const sameText of [false, true]) {
    for (const hasRevision of [false, true]) {
      for (const busy of [false, true]) {
        if (isSaved(sameText, hasRevision, busy)) accepted.push([sameText, hasRevision, busy]);
      }
    }
  }
  assert.deepEqual(accepted, [[true, true, false]]);
});

test("remote reads and applications reject every unsafe editor state", () => {
  for (let mask = 0; mask < 256; mask++) {
    const flag = (bit: number): boolean => Boolean(mask & (1 << bit));
    assert.equal(
      mayReadRemote(flag(0), flag(1), flag(2), flag(3), flag(4), flag(5), flag(6)),
      (mask & 127) === 3,
    );
    assert.equal(
      mayApplyRemote(flag(0), flag(1), flag(2), flag(3), flag(4), flag(5), flag(6), flag(7)),
      mask === 7,
    );
  }
  assert.equal(mayReadRemote(true, true, false, false, false, false, false), true);
  assert.equal(mayApplyRemote(true, true, true, false, false, false, false, false), true);
});

test("recovery distinguishes active owners and explicit uncoordinated recovery", () => {
  for (let mask = 0; mask < 16; mask++) {
    const flag = (bit: number): boolean => Boolean(mask & (1 << bit));
    assert.equal(
      mayRecoverDraft(flag(0), flag(1), flag(2), flag(3)),
      [3, 5, 7, 11, 15].includes(mask),
    );
    assert.equal(hasRemoteConflict(flag(0), flag(1)), (mask & 3) === 1);
  }
  assert.equal(mayRecoverDraft(true, false, true, true), false);
  assert.equal(mayRecoverDraft(true, false, false, false), false);
  assert.equal(mayRecoverDraft(false, true, true, false), false);
  assert.equal(mayRecoverDraft(true, false, true, false), true);
  assert.equal(mayRecoverDraft(true, true, false, false), true);
  assert.equal(hasRemoteConflict(true, false), true);
  assert.equal(hasRemoteConflict(true, true), false);
  assert.equal(hasRemoteConflict(false, false), false);
});

test("formatting preserves prose wrapping, code bytes, and cursor bounds", async () => {
  const source = "# T\n\n*   one\n*   two\n\n```txt\n  exact    spacing\n```\n";
  const formatted = await formatMarkdown(source);
  assert.ok(formatted.includes("- one\n- two"));
  assert.ok(formatted.includes("  exact    spacing"));
  assert.equal(await formatMarkdown(formatted), formatted);
  assert.equal(await formatMarkdown(""), "");
  const large = `# Unformatted\n\n${"*   literal source\n".repeat(4000)}`;
  assert.equal(await formatMarkdown(large), large);
  for (let cursor = 0; cursor <= source.length; cursor++) {
    const mapped = mappedCursor(source, formatted, cursor);
    assert.ok(mapped >= 0 && mapped <= formatted.length);
  }
});

test("note titles never depend on storage identifiers", () => {
  assert.equal(noteTitle("# A title\n\nBody"), "A title");
  assert.equal(noteTitle("Paragraph\nnext"), "Paragraph");
  assert.equal(noteTitle(""), "Untitled note");
});

test("slash completion respects line boundaries, selections, and fenced code", () => {
  assert.deepEqual(slashRange("  /image", 8, 8), { start: 2, end: 8, query: "image" });
  assert.equal(slashRange("https://example.test/image", 26, 26), null);
  assert.equal(slashRange("/image", 0, 6), null);
  assert.equal(slashRange("    /image", 10, 10), null);
  const fenced = "```text\n/image";
  assert.equal(slashRange(fenced, fenced.length, fenced.length), null);
  const closed = "~~~\ncode\n~~~\n/image";
  assert.deepEqual(slashRange(closed, closed.length, closed.length), {
    start: closed.length - 6,
    end: closed.length,
    query: "image",
  });
  assert.deepEqual(
    matchingCommands("heading").map((command) => command.id),
    ["heading", "heading2", "heading3"],
  );
  assert.deepEqual(matchingCommands("unknown"), []);
});

test("typing edits preserve selections, links, and paired characters", () => {
  const blank = { text: "", start: 0, end: 0 };
  assert.deepEqual(completeInput(blank, "*"), { text: "**", start: 1, end: 1 });
  assert.deepEqual(completeInput({ text: "**", start: 1, end: 1 }, "*", true), {
    text: "****",
    start: 2,
    end: 2,
  });
  assert.deepEqual(completeInput(blank, "["), { text: "[]()", start: 1, end: 1 });
  assert.deepEqual(completeInput({ text: "label", start: 0, end: 5 }, "["), {
    text: "[label]()",
    start: 8,
    end: 8,
  });
  assert.deepEqual(completeInput({ text: "[label]()", start: 6, end: 6 }, "]"), {
    text: "[label]()",
    start: 8,
    end: 8,
  });
  assert.deepEqual(removePair({ text: "[]()", start: 1, end: 1 }), blank);
  assert.deepEqual(wrapSelection({ text: "**abc**", start: 2, end: 5 }, "**"), {
    text: "abc",
    start: 0,
    end: 3,
  });
  assert.equal(completeInput({ text: "dont", start: 3, end: 3 }, "'"), null);
  assert.equal(completeInput(blank, "変換"), null);
  assert.deepEqual(completeInput({ text: "**bold**", start: 7, end: 7 }, "*"), {
    text: "**bold**",
    start: 8,
    end: 8,
  });
});

test("Markdown lines continue and selected lines indent as one edit", () => {
  for (const [source, result] of [
    ["- item", "- item\n- "],
    ["- [x] done", "- [x] done\n- [ ] "],
    ["9. item", "9. item\n10. "],
    ["> quote", "> quote\n> "],
    ["- ", ""],
  ]) {
    assert.ok(source !== undefined && result !== undefined);
    const edit = continueLine({ text: source, start: source.length, end: source.length });
    assert.equal(edit?.text, result);
    assert.equal(edit?.start, result.length);
  }
  assert.deepEqual(indentLines({ text: "one\ntwo", start: 0, end: 7 }, 4, false), {
    text: "    one\n    two",
    start: 4,
    end: 15,
  });
  assert.deepEqual(indentLines({ text: "    one\n    two", start: 4, end: 15 }, 4, true), {
    text: "one\ntwo",
    start: 0,
    end: 7,
  });
  assert.equal(continueLine({ text: "\nfirst", start: 0, end: 0 }), null);
  assert.deepEqual(indentLines({ text: "\nfirst", start: 0, end: 0 }, 4, false), {
    text: "    \nfirst",
    start: 4,
    end: 4,
  });
});
