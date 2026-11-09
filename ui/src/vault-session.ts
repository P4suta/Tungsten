let unlock: (() => Promise<void>) | null = null;
let pending: Promise<void> | null = null;

export function configureVaultUnlock(operation: () => Promise<void>): void {
  unlock = operation;
}

export async function vaultFetch(path: string, options: RequestInit): Promise<Response> {
  const response = await fetch(path, options);
  if (response.status !== 401 || !unlock) return response;
  await response.body?.cancel();
  pending ??= unlock().finally(() => {
    pending = null;
  });
  await pending;
  options.signal?.throwIfAborted();
  return fetch(path, options);
}
