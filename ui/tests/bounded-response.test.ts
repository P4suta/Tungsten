import assert from "node:assert/strict";
import { test } from "node:test";
import { responseBytes } from "../src/bounded-response.ts";

test("bounds responses, rejects truncated bodies, and cancels failed streams", async () => {
  let cancelled = 0;
  const stream = (): ReadableStream<Uint8Array<ArrayBuffer>> =>
    new ReadableStream({
      pull(controller) {
        controller.enqueue(new Uint8Array(1024));
      },
      cancel() {
        cancelled++;
      },
    });
  const signal = AbortSignal.timeout(1000);
  await assert.rejects(
    responseBytes(
      new Response(stream(), { headers: { "content-length": "99999999" } }),
      2048,
      signal,
    ),
    /limit/,
  );
  await assert.rejects(responseBytes(new Response(stream()), 2048, signal), /limit/);
  assert.equal(cancelled, 2);
  await assert.rejects(
    responseBytes(
      new Response(new Uint8Array(17), { headers: { "content-length": "19" } }),
      32,
      signal,
    ),
    /incomplete/,
  );
  const bytes = new Uint8Array([0, 127, 255]);
  assert.deepEqual(await responseBytes(new Response(bytes), 32, signal), bytes);
  await assert.rejects(responseBytes(new Response(bytes), 32, AbortSignal.abort()), /abort/i);
});
