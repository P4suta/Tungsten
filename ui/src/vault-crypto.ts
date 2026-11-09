export type VaultPurpose = "root" | "check" | "record" | "chunk" | "page" | "draft" | "tree";

export interface VaultContext {
  vault: string;
  key: string;
  purpose: VaultPurpose;
  object: string;
  position: number;
}

export interface VaultManifest {
  format: 1;
  id: string;
  key: string;
  root: string;
  check: string;
}

export interface VaultAccess {
  manifest: VaultManifest;
  recovery: string;
  key: CryptoKey;
}

export const vaultObjectLimit = 2 * 1024 * 1024;

const magic = new Uint8Array([84, 86, 76, 84, 1]);
const headerLength = magic.length + 16 + 12;
const recoveryPrefix = "tungsten-recovery-1:";
const marker = new TextEncoder().encode("Tungsten vault key");
const purposes = new Set<VaultPurpose>([
  "root",
  "check",
  "record",
  "chunk",
  "page",
  "draft",
  "tree",
]);
const identifier = /^[a-f0-9]{32}$/;

function nativeCrypto(): Crypto {
  if (!globalThis.crypto?.subtle) throw new Error("Use HTTPS or localhost to unlock the vault.");
  return globalThis.crypto;
}

function encode(bytes: Uint8Array<ArrayBuffer>): string {
  let text = "";
  for (const byte of bytes) text += String.fromCharCode(byte);
  return btoa(text).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
}

function decode(text: string): Uint8Array<ArrayBuffer> {
  if (text.length > 512 || !/^[A-Za-z0-9_-]+$/.test(text) || text.length % 4 === 1)
    throw new Error("Vault data is invalid.");
  const padded = text
    .replaceAll("-", "+")
    .replaceAll("_", "/")
    .padEnd(Math.ceil(text.length / 4) * 4, "=");
  const bytes = Uint8Array.from(atob(padded), (character) => character.charCodeAt(0));
  if (encode(bytes) !== text) throw new Error("Vault data is invalid.");
  return bytes;
}

function random(length: number): Uint8Array<ArrayBuffer> {
  return nativeCrypto().getRandomValues(new Uint8Array(length));
}

function rawKey(bytes: Uint8Array<ArrayBuffer>): Promise<CryptoKey> {
  if (bytes.byteLength !== 32) throw new Error("The vault key is invalid.");
  return nativeCrypto().subtle.importKey("raw", bytes, "HKDF", false, ["deriveKey"]);
}

function associatedData(context: VaultContext): Uint8Array<ArrayBuffer> {
  if (
    !identifier.test(context.vault) ||
    !identifier.test(context.key) ||
    !purposes.has(context.purpose) ||
    !/^[a-z0-9][a-z0-9.-]{0,127}$/.test(context.object) ||
    !Number.isSafeInteger(context.position) ||
    context.position < 0
  )
    throw new Error("The vault object identity is invalid.");
  return new TextEncoder().encode(
    JSON.stringify([
      "tungsten-vault",
      1,
      context.vault,
      context.key,
      context.purpose,
      context.object,
      context.position,
    ]),
  );
}

function objectKey(
  key: CryptoKey,
  salt: Uint8Array<ArrayBuffer>,
  info: Uint8Array<ArrayBuffer>,
): Promise<CryptoKey> {
  if (key.extractable || key.algorithm.name !== "HKDF" || !key.usages.includes("deriveKey"))
    throw new Error("The vault key is invalid.");
  return nativeCrypto().subtle.deriveKey(
    { name: "HKDF", hash: "SHA-256", salt, info },
    key,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt", "decrypt"],
  );
}

export async function sealObject(
  key: CryptoKey,
  context: VaultContext,
  data: Uint8Array<ArrayBuffer>,
): Promise<Uint8Array<ArrayBuffer>> {
  if (data.byteLength > vaultObjectLimit) throw new Error("The vault object is too large.");
  const plain = data.slice();
  const info = associatedData(context);
  const salt = random(16);
  const iv = random(12);
  try {
    const derived = await objectKey(key, salt, info);
    const cipher = await nativeCrypto().subtle.encrypt(
      { name: "AES-GCM", iv, additionalData: info, tagLength: 128 },
      derived,
      plain,
    );
    const result = new Uint8Array(headerLength + cipher.byteLength);
    result.set(magic);
    result.set(salt, magic.length);
    result.set(iv, magic.length + salt.length);
    result.set(new Uint8Array(cipher), headerLength);
    return result;
  } finally {
    plain.fill(0);
  }
}

export async function openObject(
  key: CryptoKey,
  context: VaultContext,
  ciphertext: Uint8Array<ArrayBuffer>,
): Promise<Uint8Array<ArrayBuffer>> {
  if (
    ciphertext.byteLength < headerLength + 16 ||
    ciphertext.byteLength > headerLength + 16 + vaultObjectLimit ||
    !magic.every((byte, index) => ciphertext[index] === byte)
  )
    throw new Error("Vault data is invalid.");
  const bytes = ciphertext.slice();
  const info = associatedData(context);
  const salt = bytes.slice(magic.length, magic.length + 16);
  const derived = await objectKey(key, salt, info);
  return new Uint8Array(
    await nativeCrypto().subtle.decrypt(
      {
        name: "AES-GCM",
        iv: bytes.slice(magic.length + 16, headerLength),
        additionalData: info,
        tagLength: 128,
      },
      derived,
      bytes.subarray(headerLength),
    ),
  );
}

function keyContext(manifest: VaultManifest, purpose: "root" | "check"): VaultContext {
  return { vault: manifest.id, key: manifest.key, purpose, object: "vault", position: 0 };
}

export function vaultManifest(value: unknown): VaultManifest {
  if (
    typeof value !== "object" ||
    value === null ||
    !("format" in value) ||
    value.format !== 1 ||
    !("id" in value) ||
    typeof value.id !== "string" ||
    !identifier.test(value.id) ||
    !("key" in value) ||
    typeof value.key !== "string" ||
    !identifier.test(value.key) ||
    !("root" in value) ||
    typeof value.root !== "string" ||
    !("check" in value) ||
    typeof value.check !== "string"
  )
    throw new Error("The vault manifest is invalid.");
  return { format: 1, id: value.id, key: value.key, root: value.root, check: value.check };
}

export async function createVault(): Promise<VaultAccess> {
  const raw = random(32);
  const recovery = random(32);
  try {
    const key = await rawKey(raw);
    const wrapping = await rawKey(recovery);
    const hex = (bytes: Uint8Array<ArrayBuffer>): string =>
      Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
    const manifest: VaultManifest = {
      format: 1,
      id: hex(random(16)),
      key: hex(random(16)),
      root: "",
      check: "",
    };
    manifest.root = encode(await sealObject(wrapping, keyContext(manifest, "root"), raw));
    manifest.check = encode(await sealObject(key, keyContext(manifest, "check"), marker));
    return { manifest, key, recovery: recoveryPrefix + encode(recovery) };
  } finally {
    raw.fill(0);
    recovery.fill(0);
  }
}

export async function unlockVault(value: unknown, recovery: string): Promise<CryptoKey> {
  const manifest = vaultManifest(value);
  if (!recovery.trim().startsWith(recoveryPrefix)) throw new Error("The recovery key is invalid.");
  const secret = decode(recovery.trim().slice(recoveryPrefix.length));
  let raw: Uint8Array<ArrayBuffer> | null = null;
  try {
    const wrapping = await rawKey(secret);
    raw = await openObject(wrapping, keyContext(manifest, "root"), decode(manifest.root));
    const key = await rawKey(raw);
    await verifyVaultKey(manifest, key);
    return key;
  } catch {
    throw new Error("Could not unlock the vault with this recovery key.");
  } finally {
    secret.fill(0);
    raw?.fill(0);
  }
}

export async function verifyVaultKey(value: unknown, key: CryptoKey): Promise<VaultManifest> {
  const manifest = vaultManifest(value);
  const check = await openObject(key, keyContext(manifest, "check"), decode(manifest.check));
  if (check.length !== marker.length || !marker.every((byte, index) => check[index] === byte))
    throw new Error("The vault key is invalid.");
  return manifest;
}
