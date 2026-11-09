import assert from "node:assert/strict";
import { test } from "node:test";
import { configureVaultUnlock, vaultFetch } from "../src/vault-session.ts";

test("coalesces concurrent renewals, preserves cancellation, and retries each request once", async (t) => {
  let release: (() => void) | undefined;
  let started: (() => void) | undefined;
  const renewing = new Promise<void>((resolve) => {
    started = resolve;
  });
  let renewals = 0;
  let active = false;
  let calls = 0;
  t.mock.method(globalThis, "fetch", async () => {
    calls++;
    return new Response(null, { status: active ? 200 : 401 });
  });
  configureVaultUnlock(async () => {
    renewals++;
    started?.();
    await new Promise<void>((resolve) => {
      release = resolve;
    });
    active = true;
  });
  const stopped = new AbortController();
  const first = vaultFetch("/api/notes", {});
  const second = vaultFetch("/api/graph", { signal: stopped.signal });
  await renewing;
  stopped.abort();
  release?.();
  assert.equal((await first).status, 200);
  await assert.rejects(second, /abort/i);
  assert.equal(renewals, 1);
  assert.equal(calls, 3);
});

test("leaves failed renewal retryable and never loops on a second denial", async (t) => {
  let renewals = 0;
  t.mock.method(globalThis, "fetch", async () => new Response(null, { status: 401 }));
  configureVaultUnlock(async () => {
    if (++renewals === 1) throw new Error("Device key unavailable");
  });
  await assert.rejects(vaultFetch("/api/notes", {}), /Device key unavailable/);
  assert.equal((await vaultFetch("/api/notes", {})).status, 401);
  assert.equal(renewals, 2);
});
