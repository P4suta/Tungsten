import { type VaultManifest, verifyVaultKey } from "./vault-crypto.ts";

export interface RememberedVault {
  key: CryptoKey;
}

interface DeviceRecord {
  vault: string;
  keyId: string;
  key: CryptoKey;
}

function device(value: unknown, manifest: VaultManifest): DeviceRecord | null {
  if (value === undefined) return null;
  if (
    typeof value !== "object" ||
    value === null ||
    !("vault" in value) ||
    value.vault !== manifest.id ||
    !("keyId" in value) ||
    value.keyId !== manifest.key ||
    !("key" in value) ||
    !(value.key instanceof CryptoKey) ||
    value.key.extractable ||
    value.key.algorithm.name !== "HKDF"
  )
    throw new Error("The remembered browser key does not match the vault.");
  return {
    vault: manifest.id,
    keyId: manifest.key,
    key: value.key,
  };
}

function database(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open("tungsten.vault.devices", 1);
    let settled = false;
    const fail = (): void => {
      settled = true;
      clearTimeout(timer);
      reject(new Error("Could not open browser key storage."));
    };
    const timer = setTimeout(fail, 5000);
    request.onupgradeneeded = () => {
      request.result.createObjectStore("devices", { keyPath: "vault" });
    };
    request.onerror = fail;
    request.onblocked = fail;
    request.onsuccess = () => {
      if (settled) {
        request.result.close();
        return;
      }
      settled = true;
      clearTimeout(timer);
      request.result.onversionchange = () => request.result.close();
      resolve(request.result);
    };
  });
}

async function record<T>(
  vault: string,
  mode: IDBTransactionMode,
  action: (store: IDBObjectStore, value: unknown) => T,
): Promise<T> {
  const db = await database();
  try {
    return await new Promise<T>((resolve, reject) => {
      const transaction = db.transaction("devices", mode, { durability: "strict" });
      const store = transaction.objectStore("devices");
      const request = store.get(vault);
      let result: T;
      let failure: unknown = new Error("Could not update browser key storage.");
      const timer = setTimeout(() => {
        try {
          transaction.abort();
        } catch {
          reject(failure);
        }
      }, 5000);
      transaction.onabort = () => {
        clearTimeout(timer);
        reject(failure);
      };
      transaction.onerror = () => {
        clearTimeout(timer);
        reject(failure);
      };
      transaction.oncomplete = () => {
        clearTimeout(timer);
        resolve(result);
      };
      request.onsuccess = () => {
        try {
          const value: unknown = request.result;
          result = action(store, value);
        } catch (error) {
          failure = error;
          transaction.abort();
        }
      };
    });
  } finally {
    db.close();
  }
}

export async function rememberedVault(manifest: VaultManifest): Promise<RememberedVault | null> {
  const saved = await record(manifest.id, "readonly", (_store, value) => device(value, manifest));
  if (!saved) return null;
  await verifyVaultKey(manifest, saved.key);
  return { key: saved.key };
}

export async function rememberVaultKey(manifest: VaultManifest, key: CryptoKey): Promise<void> {
  await verifyVaultKey(manifest, key);
  await record(manifest.id, "readwrite", (store, value) => {
    device(value, manifest);
    const saved: DeviceRecord = {
      vault: manifest.id,
      keyId: manifest.key,
      key,
    };
    store.put(saved);
  });
}

export async function forgetVault(manifest: VaultManifest): Promise<void> {
  await record(manifest.id, "readwrite", (store) => {
    store.delete(manifest.id);
  });
}
