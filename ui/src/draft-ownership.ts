import { identifier } from "./identity.ts";

export const uncoordinatedDraftOwner = "uncoordinated";
export let draftOwner = uncoordinatedDraftOwner;
const documentOwner = identifier();
const prefix = "tungsten.draft-owner.";
let ready: Promise<void> | null = null;

export async function activeDraftOwners(): Promise<ReadonlySet<string> | null> {
  try {
    if (!navigator.locks) return null;
    ready ??= new Promise<void>((resolve) => {
      void navigator.locks
        .request(prefix + documentOwner, { signal: AbortSignal.timeout(1000) }, () => {
          draftOwner = documentOwner;
          resolve();
          return new Promise<void>(() => {});
        })
        .catch(() => resolve());
    });
    await ready;
    const locks = await navigator.locks.query();
    if (!locks.held) return null;
    return new Set([
      uncoordinatedDraftOwner,
      ...locks.held.flatMap((lock) =>
        lock.name?.startsWith(prefix) ? [lock.name.slice(prefix.length)] : [],
      ),
    ]);
  } catch {
    return null;
  }
}
