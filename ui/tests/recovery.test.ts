import assert from "node:assert/strict";
import { test } from "node:test";
import { versionDifference } from "../src/differences.ts";
import { savedEntries } from "../src/recovery.ts";

test("recovery comparisons preserve Unicode and literal HTML while bounding work and output", () => {
  const change = versionDifference("Keep\nOld 日本語\n", "Keep\nNew <script>literal</script>\n");
  assert.equal(change?.added, 1);
  assert.equal(change?.removed, 1);
  assert.equal(change?.text, "  Keep\n- Old 日本語\n+ New <script>literal</script>");
  assert.equal(change?.complete, true);
  assert.equal(versionDifference("old\n", "new\n", 0), null);
  assert.equal(versionDifference("x".repeat(256 * 1024), "different"), null);
  const abbreviated = versionDifference("", "line\n".repeat(300));
  assert.equal(abbreviated?.added, 300);
  assert.equal(abbreviated?.complete, false);
  assert.ok((abbreviated?.text.length ?? Infinity) < 16 * 1024);
  const context = versionDifference(
    `Start\n${"same\n".repeat(50)}End\n`,
    `Begin\n${"same\n".repeat(50)}End\n`,
  );
  assert.match(context?.text ?? "", /unchanged lines/);
  assert.deepEqual(
    savedEntries([
      { id: "id", name: "name", modified: 1, revision: "revision", bytes: 4, title: "Title" },
    ])[0]?.captured,
    1,
  );
  for (const invalid of [null, {}, [null], [{ id: "id" }]])
    assert.throws(() => savedEntries(invalid));
});
