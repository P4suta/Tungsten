import { responseBytes } from "./bounded-response.js";
import { icon } from "./icons.js";
import { encodeSeed, type RuntimeManifest, runtimeAccess, runtimeManifest } from "./server-key.ts";
import { createVault, unlockVault } from "./vault-crypto.ts";
import { forgetVault, rememberedVault, rememberVaultKey } from "./vault-devices.ts";
import { configureVaultUnlock } from "./vault-session.js";

interface VaultStatus {
  manifest: RuntimeManifest | null;
  unlocked: boolean;
  opening: boolean;
  failed: boolean;
}

async function status(): Promise<VaultStatus> {
  const signal = AbortSignal.timeout(15000);
  const response = await fetch("/api/vault", {
    cache: "no-store",
    signal,
    redirect: "error",
  });
  const text = new TextDecoder("utf-8", { fatal: true }).decode(
    await responseBytes(response, 4096, signal),
  );
  const value: unknown = JSON.parse(text);
  if (
    typeof value !== "object" ||
    value === null ||
    !("manifest" in value) ||
    !("unlocked" in value) ||
    typeof value.unlocked !== "boolean" ||
    !("opening" in value) ||
    typeof value.opening !== "boolean" ||
    !("failed" in value) ||
    typeof value.failed !== "boolean"
  )
    throw new Error("The vault response is invalid.");
  return {
    manifest: value.manifest === null ? null : runtimeManifest(value.manifest),
    unlocked: value.unlocked,
    opening: value.opening,
    failed: value.failed,
  };
}

async function connect(
  manifest: RuntimeManifest,
  key: CryptoKey,
  initialize = false,
): Promise<void> {
  const access = await runtimeAccess(manifest.vault, key, manifest.phase);
  try {
    if (access.manifest.check !== manifest.check)
      throw new Error("The vault does not match the browser key.");
    const response = await fetch(initialize ? "/api/vault" : "/api/vault/unlock", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ ...(initialize ? { manifest } : {}), seed: encodeSeed(access.seed) }),
      cache: "no-store",
      redirect: "error",
      signal: AbortSignal.timeout(15000),
    });
    await response.body?.cancel();
    if (response.status !== 204 && response.status !== 202)
      throw new Error("Could not unlock the server. Keep the recovery key and retry.");
    let current = await status();
    while (current.opening) {
      const notice = document.getElementById("vault-entry-notice");
      if (notice)
        notice.textContent =
          "Copying notes into encrypted storage. Keep the original directory until you verify the copy.";
      await new Promise<void>((resolve) => setTimeout(resolve, 500));
      current = await status();
    }
    if (current.failed)
      throw new Error(
        "Migration stopped. Keep both directories and retry with the same source and recovery key.",
      );
    if (!current.unlocked) throw new Error("Use HTTPS to remember the vault session.");
  } finally {
    access.seed.fill(0);
  }
}

function page(
  title: string,
  description: string,
): { root: HTMLElement; form: HTMLFormElement; notice: HTMLParagraphElement } {
  document.body.classList.add("vault-locked");
  document.getElementById("vault-entry")?.remove();
  const root = document.createElement("main");
  root.id = "vault-entry";
  root.className = "vault-entry";
  const heading = document.createElement("h1");
  heading.textContent = title;
  heading.tabIndex = -1;
  const help = document.createElement("p");
  help.textContent = description;
  const form = document.createElement("form");
  const notice = document.createElement("p");
  notice.id = "vault-entry-notice";
  notice.setAttribute("role", "status");
  notice.setAttribute("aria-live", "polite");
  root.append(heading, help, form, notice);
  document.body.prepend(root);
  heading.focus();
  return { root, form, notice };
}

function button(text: string, kind: "lock" | "copy" | "edit" | "undo"): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.append(icon(kind), document.createTextNode(text));
  return button;
}

function field(form: HTMLFormElement, secret: string, readonly: boolean): HTMLInputElement {
  const label = document.createElement("label");
  label.htmlFor = "vault-recovery";
  label.textContent = "Recovery key";
  const wrapper = document.createElement("div");
  wrapper.className = "vault-key-field";
  const input = document.createElement("input");
  input.id = label.htmlFor;
  input.type = readonly ? "text" : "password";
  input.autocomplete = readonly ? "off" : "current-password";
  input.setAttribute("autocapitalize", "none");
  input.spellcheck = false;
  input.readOnly = readonly;
  input.maxLength = 96;
  input.required = true;
  input.setAttribute("aria-describedby", "vault-entry-notice");
  input.value = secret;
  wrapper.append(icon("lock"), input);
  form.append(label, wrapper);
  return input;
}

async function enrollment(): Promise<RuntimeManifest> {
  const access = await createVault();
  const runtime = await runtimeAccess(access.manifest, access.key);
  runtime.seed.fill(0);
  const screen = page(
    "Save your recovery key",
    "Keep the key in your password manager. Use it to unlock another browser or recover after clearing browser data.",
  );
  const input = field(screen.form, access.recovery, true);
  const copy = button("Copy recovery key", "copy");
  copy.addEventListener("click", () => {
    void Promise.resolve()
      .then(() => navigator.clipboard.writeText(input.value))
      .then(() => {
        screen.notice.textContent = "Recovery key copied.";
      })
      .catch(() => {
        input.focus();
        input.select();
        screen.notice.textContent = "Select and copy the recovery key.";
      });
  });
  const label = document.createElement("label");
  label.className = "vault-confirm";
  const saved = document.createElement("input");
  saved.type = "checkbox";
  label.append(saved, document.createTextNode("I saved the recovery key"));
  const submit = button("Start writing", "edit");
  submit.type = "submit";
  submit.disabled = true;
  saved.addEventListener("change", () => {
    submit.disabled = !saved.checked;
  });
  screen.form.append(copy, label, submit);
  return new Promise<RuntimeManifest>((resolve) => {
    screen.form.addEventListener("submit", (event) => {
      event.preventDefault();
      if (!saved.checked || submit.disabled) return;
      submit.disabled = true;
      saved.disabled = true;
      screen.notice.textContent = "Preparing encrypted storage…";
      void (async () => {
        try {
          await rememberVaultKey(access.manifest, access.key);
          await connect(runtime.manifest, access.key, true);
          input.value = "";
          access.recovery = "";
          screen.root.remove();
          resolve(runtime.manifest);
        } catch (error) {
          screen.notice.textContent =
            error instanceof Error
              ? error.message
              : "Could not prepare encrypted storage. Keep the recovery key and retry.";
          submit.disabled = false;
          saved.disabled = false;
        }
      })();
    });
  });
}

async function unlock(manifest: RuntimeManifest): Promise<void> {
  let damaged = false;
  const saved = await rememberedVault(manifest.vault).catch(() => {
    damaged = true;
    return null;
  });
  if (saved) {
    await connect(manifest, saved.key);
    return;
  }
  const screen = page(
    "Unlock notes",
    "Paste your recovery key. Access will be remembered in this browser.",
  );
  const input = field(screen.form, "", false);
  const submit = button("Unlock notes", "lock");
  submit.type = "submit";
  screen.form.append(submit);
  input.focus();
  return new Promise<void>((resolve) => {
    screen.form.addEventListener("submit", (event) => {
      event.preventDefault();
      if (submit.disabled) return;
      submit.disabled = true;
      screen.notice.textContent = "Unlocking…";
      void (async () => {
        try {
          const key = await unlockVault(manifest.vault, input.value.trim()).catch(() => {
            throw new Error(
              "The recovery key did not unlock the notes. Check the key and try again.",
            );
          });
          await connect(manifest, key);
          if (damaged) await forgetVault(manifest.vault);
          await rememberVaultKey(manifest.vault, key);
          input.value = "";
          screen.root.remove();
          resolve();
        } catch (error) {
          input.setAttribute("aria-invalid", "true");
          screen.notice.textContent =
            error instanceof Error ? error.message : "The recovery key did not unlock the notes.";
          submit.disabled = false;
          input.focus();
        }
      })();
    });
  });
}

async function start(): Promise<void> {
  const state = await status();
  const manifest = state.manifest ?? (await enrollment());
  if (state.manifest) {
    if (!state.unlocked) await unlock(manifest);
  }
  configureVaultUnlock(async () => {
    const current = await status();
    if (
      !current.manifest ||
      current.manifest.vault.id !== manifest.vault.id ||
      current.manifest.vault.key !== manifest.vault.key
    )
      throw new Error("The server vault changed. Keep your draft and reload.");
    const remembered = await rememberedVault(current.manifest.vault);
    if (!remembered)
      throw new Error("The browser key is missing. Reload to unlock; your draft is kept.");
    await connect(current.manifest, remembered.key);
  });
  const file = new URL(location.href).searchParams.get("file");
  if (file && /^[a-f0-9]{32}\.[a-zA-Z0-9]{1,12}$/.test(file)) {
    location.replace(`/.attachments/${file}`);
    return;
  }
  document.body.classList.remove("vault-locked");
  const entry = "/assets/app.js";
  await import(entry);
}

void start().catch((error: unknown) => {
  const screen = page(
    "Could not open notes",
    error instanceof Error ? error.message : "The encrypted vault is unavailable.",
  );
  const retry = button("Retry", "undo");
  retry.addEventListener("click", () => location.reload());
  screen.form.append(retry);
});
