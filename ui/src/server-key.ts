import { type VaultManifest, vaultManifest, verifyVaultKey } from "./vault-crypto.ts";

export interface RuntimeManifest {
  format: 1;
  phase: "pending" | "migrating" | "ready";
  vault: VaultManifest;
  check: string;
}

export function runtimeManifest(value: unknown): RuntimeManifest {
  if (
    typeof value !== "object" ||
    value === null ||
    !("format" in value) ||
    value.format !== 1 ||
    !("phase" in value) ||
    (value.phase !== "pending" && value.phase !== "migrating" && value.phase !== "ready") ||
    !("vault" in value) ||
    !("check" in value) ||
    typeof value.check !== "string" ||
    !/^[A-Za-z0-9_-]{43}$/.test(value.check)
  )
    throw new Error("The server vault manifest is invalid.");
  return { format: 1, phase: value.phase, vault: vaultManifest(value.vault), check: value.check };
}

export function encodeSeed(bytes: Uint8Array<ArrayBuffer>): string {
  let text = "";
  for (const byte of bytes) text += String.fromCharCode(byte);
  return btoa(text).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
}

export async function runtimeAccess(
  vault: VaultManifest,
  key: CryptoKey,
  phase: RuntimeManifest["phase"] = "pending",
): Promise<{ seed: Uint8Array<ArrayBuffer>; manifest: RuntimeManifest }> {
  await verifyVaultKey(vault, key);
  const encoder = new TextEncoder();
  const salt = encoder.encode(vault.id);
  const derived = await crypto.subtle.deriveKey(
    {
      name: "HKDF",
      hash: "SHA-256",
      salt,
      info: encoder.encode(JSON.stringify(["tungsten-server-seed", 1, vault.id, vault.key])),
    },
    key,
    { name: "HMAC", hash: "SHA-256", length: 256 },
    true,
    ["sign"],
  );
  const seed = new Uint8Array(await crypto.subtle.exportKey("raw", derived));
  try {
    const root = await crypto.subtle.importKey("raw", seed, "HKDF", false, ["deriveKey"]);
    const verification = await crypto.subtle.deriveKey(
      {
        name: "HKDF",
        hash: "SHA-256",
        salt,
        info: encoder.encode("tungsten.runtime-verification.1"),
      },
      root,
      { name: "HMAC", hash: "SHA-256", length: 256 },
      false,
      ["sign"],
    );
    const check = encodeSeed(
      new Uint8Array(
        await crypto.subtle.sign(
          "HMAC",
          verification,
          encoder.encode(
            `tungsten.server-check.1:${vault.id}:${vault.key}:${vault.root}:${vault.check}:${phase}`,
          ),
        ),
      ),
    );
    return { seed, manifest: { format: 1, phase, vault, check } };
  } catch (error) {
    seed.fill(0);
    throw error;
  }
}
