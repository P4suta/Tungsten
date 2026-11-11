import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import { versionDifference } from "../src/differences.ts";
import { savedEntries } from "../src/recovery.ts";

test("recovery comparisons preserve Unicode and literal HTML while bounding work and output", (testContext: TestContext) => {
  testContext.mock.method(Date, "now", (): number => 1000);
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

test("recovery comparisons distinguish the deadline boundary from expired work", (testContext: TestContext) => {
  for (const elapsed of [40, 41]) {
    let reads: number = 0;
    testContext.mock.method(Date, "now", (): number => (reads++ === 0 ? 1000 : 1000 + elapsed));
    const change = versionDifference("old\n", "new\n");
    if (elapsed === 40) {
      assert.equal(change?.added, 1);
      assert.equal(change?.removed, 1);
    } else {
      assert.equal(change, null);
    }
    testContext.mock.restoreAll();
  }
});
