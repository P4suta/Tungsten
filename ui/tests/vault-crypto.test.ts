import assert from "node:assert/strict";
import { test } from "node:test";
import {
  createVault,
  openObject,
  sealObject,
  unlockVault,
  type VaultContext,
  vaultObjectLimit,
} from "../src/vault-crypto.ts";

test("enrolls a device with a non-extractable root and an independent recovery secret", async () => {
  const access = await createVault();
  assert.equal(access.key.extractable, false);
  assert.equal(access.key.algorithm.name, "HKDF");
  assert.match(access.recovery, /^tungsten-recovery-1:[A-Za-z0-9_-]{43}$/);
  assert.equal(JSON.stringify(access.manifest).includes(access.recovery), false);
  await assert.rejects(crypto.subtle.exportKey("raw", access.key));
  const restored = await unlockVault(access.manifest, access.recovery);
  assert.equal(restored.extractable, false);
  const context: VaultContext = {
    vault: access.manifest.id,
    key: access.manifest.key,
    purpose: "record",
    object: "example-record",
    position: 0,
  };
  const original = new TextEncoder().encode("# Private writing\n\n日本語と原本\u0000\n");
  const first = await sealObject(access.key, context, original);
  const second = await sealObject(access.key, context, original);
  assert.notDeepEqual(first, second);
  assert.deepEqual(await openObject(restored, context, first), original);
  assert.equal(new TextDecoder().decode(first).includes("Private writing"), false);
});

test("rejects wrong keys, contexts, altered bytes, and incomplete ciphertext", async () => {
  const access = await createVault();
  const other = await createVault();
  const context: VaultContext = {
    vault: access.manifest.id,
    key: access.manifest.key,
    purpose: "page",
    object: "catalog",
    position: 17,
  };
  const original = new Uint8Array([0, 1, 127, 128, 255]);
  const ciphertext = await sealObject(access.key, context, original);
  await assert.rejects(unlockVault(access.manifest, other.recovery));
  await assert.rejects(openObject(other.key, context, ciphertext));
  for (const changed of [
    { vault: other.manifest.id },
    { key: other.manifest.key },
    { purpose: "chunk" as const },
    { object: "another-catalog" },
    { position: 18 },
  ])
    await assert.rejects(openObject(access.key, { ...context, ...changed }, ciphertext));
  for (let offset = 0; offset < ciphertext.length; offset++) {
    const altered = ciphertext.slice();
    const byte = altered[offset];
    assert.ok(byte !== undefined);
    altered[offset] = byte ^ 1;
    await assert.rejects(openObject(access.key, context, altered));
  }
  for (let length = 0; length < ciphertext.length; length++)
    await assert.rejects(openObject(access.key, context, ciphertext.slice(0, length)));
  assert.deepEqual(await openObject(access.key, context, ciphertext), original);
});

test("rejects malformed manifests, recovery secrets, contexts, and oversized objects", async () => {
  const access = await createVault();
  for (const malformed of [
    null,
    [],
    {},
    { ...access.manifest, format: 2 },
    {
      ...access.manifest,
      root: "not-base64!",
    },
  ])
    await assert.rejects(unlockVault(malformed, access.recovery));
  for (const recovery of ["", "password", `${access.recovery}=`, access.recovery.slice(0, -1)])
    await assert.rejects(unlockVault(access.manifest, recovery));
  const context: VaultContext = {
    vault: access.manifest.id,
    key: access.manifest.key,
    purpose: "draft",
    object: "draft",
    position: 0,
  };
  for (const invalid of [Number.NaN, -1, Number.MAX_SAFE_INTEGER + 1])
    await assert.rejects(
      sealObject(access.key, { ...context, position: invalid }, new Uint8Array()),
    );
  await assert.rejects(sealObject(access.key, context, new Uint8Array(vaultObjectLimit + 1)));
  await assert.rejects(openObject(access.key, context, new Uint8Array(vaultObjectLimit + 1024)));
  assert.deepEqual(
    await openObject(access.key, context, await sealObject(access.key, context, new Uint8Array())),
    new Uint8Array(),
  );
});
