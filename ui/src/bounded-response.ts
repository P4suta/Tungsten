export async function responseBytes(
  response: Response,
  maximum: number,
  signal: AbortSignal,
): Promise<Uint8Array<ArrayBuffer>> {
  let reader: ReadableStreamDefaultReader<Uint8Array<ArrayBuffer>> | null = null;
  try {
    if (!response.ok) throw new Error(`Vault request failed (${response.status}).`);
    const declared = response.headers.get("content-length");
    if (declared !== null && (!/^\d+$/.test(declared) || Number(declared) > maximum))
      throw new Error("The vault response exceeds its byte limit.");
    if (!response.body) throw new Error("The vault response is incomplete.");
    reader = response.body.getReader();
    const result = new Uint8Array(maximum);
    let length = 0;
    for (;;) {
      signal.throwIfAborted();
      const part = await reader.read();
      if (part.done) break;
      if (part.value.length > maximum - length)
        throw new Error("The vault response exceeds its byte limit.");
      result.set(part.value, length);
      length += part.value.length;
    }
    if (declared !== null && Number(declared) !== length)
      throw new Error("The vault response is incomplete.");
    return result.slice(0, length);
  } finally {
    if (reader) {
      await reader.cancel().catch(() => {});
      reader.releaseLock();
    } else if (response.body) {
      await response.body.cancel().catch(() => {});
    }
  }
}
