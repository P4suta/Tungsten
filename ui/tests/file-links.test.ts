import assert from "node:assert/strict";
import { test } from "node:test";
import { changedFileLinks, removeFileLinks, repairFileLinks } from "../src/file-links.ts";

test("file link changes distinguish destination edits from deliberate removal and literal code", () => {
  const before = "![Image](.attachments/abc.png)\n\n[file](.attachments/def.pdf)";
  const after = "![Image](.attachments/abd.png)\n\n[file](.attachments/def.pdf)\n\nNew text";
  const changed = changedFileLinks(before, after);
  assert.equal(changed.length, 1);
  assert.equal(changed[0]?.previous, ".attachments/abc.png");
  assert.equal(changed[0]?.destination, ".attachments/abd.png");
  assert.equal(repairFileLinks(after, changed), `${before}\n\nNew text`);
  assert.deepEqual(changedFileLinks(before, "[file](.attachments/def.pdf)"), []);
  assert.deepEqual(
    changedFileLinks(["```md", before, "```"].join("\n"), ["```md", after, "```"].join("\n")),
    [],
  );
  assert.deepEqual(changedFileLinks(before, before.replace("Image", "Description")), []);
  assert.deepEqual(
    changedFileLinks(
      before,
      before.replace(".attachments/abc.png", "https://example.com/image.png"),
    ),
    [],
  );
});

test("repair preserves unrelated edits and refuses ambiguous labels", () => {
  const before = "[same](.attachments/a.txt) [same](.attachments/b.txt)";
  assert.deepEqual(changedFileLinks(before, "[same](.attachments/c.txt)"), []);
  assert.equal(
    repairFileLinks("unrelated", [{ from: 0, to: 1, previous: "a", destination: "changed" }]),
    "unrelated",
  );
});

test("incomplete file destinations keep their original repair target", () => {
  const before = "[file](.attachments/original.pdf)";
  for (const after of [
    "[file]()",
    "[file](.attachments/original.pdf",
    "[file](.attachments/broken.pdf)",
  ])
    assert.equal(repairFileLinks(after, changedFileLinks(before, after)), before);
  assert.deepEqual(changedFileLinks(before, ""), []);
  assert.deepEqual(changedFileLinks(before, "`[file]()`"), []);
});

test("permanent file removal changes local references without touching external links or code", () => {
  const source =
    "![image](../.attachments/a.png) [file](/.attachments/a.png) [remote](https://example.com/.attachments/a.png) `![code](.attachments/a.png)`";
  assert.equal(
    removeFileLinks(source, "a.png"),
    "  [remote](https://example.com/.attachments/a.png) `![code](.attachments/a.png)`",
  );
});
