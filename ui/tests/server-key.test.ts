import assert from "node:assert/strict";
import { test } from "node:test";
import { runtimeAccess, runtimeManifest } from "../src/server-key.ts";
import { createVault } from "../src/vault-crypto.ts";

test("derives stable server access without exporting the browser master and binds its public verification", async () => {
  const access = await createVault();
  const first = await runtimeAccess(access.manifest, access.key);
  const second = await runtimeAccess(access.manifest, access.key);
  assert.equal(first.seed.byteLength, 32);
  assert.deepEqual(first, second);
  assert.equal(access.key.extractable, false);
  assert.equal(first.manifest.check.length, 43);
  assert.deepEqual(runtimeManifest(first.manifest), first.manifest);
  const other = await createVault();
  await assert.rejects(runtimeAccess(access.manifest, other.key));
  assert.throws(() => runtimeManifest({ ...first.manifest, format: 2 }));
  assert.throws(() => runtimeManifest({ ...first.manifest, check: "invalid" }));
  first.seed.fill(0);
  second.seed.fill(0);
});
