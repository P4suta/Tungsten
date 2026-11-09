import { type ChildProcess, spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { test as base, expect } from "@playwright/test";
import { accessibilityViolations } from "./accessibility.js";
import { missingIcons } from "./icons-check.js";
import { attachToNote, openTrash } from "./interface.js";

interface Server {
  origin: string;
  restart(): Promise<void>;
}

const test = base.extend<{ sealed: Server }>({
  sealed: async ({ browserName }, use) => {
    const directory = `../target/browser-sealed-${browserName}-${randomUUID()}`;
    let child: ChildProcess | null = null;
    let closed: Promise<void> = Promise.resolve();
    let origin = "";
    const start = async (): Promise<void> => {
      const address = origin ? new URL(origin).host : "127.0.0.1:0";
      const process = spawn(
        "../target/debug/tungsten",
        ["--development", "--data-dir", directory, "--listen", address],
        { stdio: ["ignore", "ignore", "pipe"] },
      );
      child = process;
      closed = new Promise<void>((resolve) => process.once("close", () => resolve()));
      origin = await new Promise<string>((resolve, reject) => {
        const timer = setTimeout(
          () => reject(new Error("The encrypted fixture did not start.")),
          10000,
        );
        let output = "";
        process.stderr.on("data", (part: Buffer) => {
          output = (output + part.toString()).slice(-16384);
          const bound = /operation=listen outcome=ready address=(127\.0\.0\.1:\d+)/.exec(
            output,
          )?.[1];
          if (bound) {
            clearTimeout(timer);
            resolve(`http://${bound}`);
          }
        });
        process.once("error", (error) => {
          clearTimeout(timer);
          reject(error);
        });
        process.once("exit", () => {
          clearTimeout(timer);
          reject(new Error("The encrypted fixture stopped."));
        });
      });
    };
    const stop = async (): Promise<void> => {
      const owned = child;
      child = null;
      if (!owned) return;
      owned.kill("SIGTERM");
      const forced = setTimeout(() => owned.kill("SIGKILL"), 2000);
      await closed;
      clearTimeout(forced);
    };
    try {
      await start();
      await use({
        get origin() {
          return origin;
        },
        restart: async () => {
          await stop();
          await start();
        },
      });
    } finally {
      await stop();
    }
  },
});

test("writes encrypted notes without an index replica and reconnects after a server restart", async ({
  page,
  context,
  sealed,
  browserName,
}) => {
  const loaded: string[] = [];
  page.on("request", (request) => loaded.push(request.url()));
  await page.goto(sealed.origin);
  await expect(page.getByRole("heading", { name: "Save your recovery key" })).toBeVisible();
  expect(await page.evaluate(missingIcons, null)).toEqual([]);
  expect(await accessibilityViolations(page)).toEqual([]);
  const recovery = await page.getByLabel("Recovery key", { exact: true }).inputValue();
  await expect(page.getByRole("button", { name: "Start writing" })).toBeDisabled();
  await page.screenshot({
    path: `../target/screenshots/vault-enrollment-${browserName}.png`,
    mask: [page.getByLabel("Recovery key", { exact: true })],
    maskColor: "#000",
  });
  await page.getByLabel("I saved the recovery key").check();
  await page.getByRole("button", { name: "Start writing" }).click();
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await expect(editor).toBeEditable();
  await editor.fill("# Private philosophy\n\n日本語のノートと **important thoughts**");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const address = page.url();
  await sealed.restart();
  await editor.press("ControlOrMeta+End");
  await editor.pressSequentially(" continued");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await page.reload();
  await expect(editor).toHaveText(/continued/);
  await expect(page.getByRole("heading", { name: "Unlock notes" })).toHaveCount(0);
  expect(
    loaded.some(
      (url) => url.endsWith("/assets/vault.wasm") || url.endsWith("/assets/markdown.wasm"),
    ),
  ).toBe(false);
  const second = await context.browser()?.newContext();
  if (!second) throw new Error("The second browser context is unavailable.");
  try {
    const other = await second.newPage();
    await other.goto(address);
    await expect(other.getByRole("heading", { name: "Unlock notes" })).toBeVisible();
    const denied = await other.request.get(`${sealed.origin}/api/notes`);
    expect(denied.status()).toBe(401);
    await other
      .getByLabel("Recovery key", { exact: true })
      .fill(`tungsten-recovery-1:${"A".repeat(43)}`);
    await other.getByRole("button", { name: "Unlock notes", exact: true }).click();
    await expect(other.getByRole("status")).toContainText("did not unlock");
    expect((await other.request.get(`${sealed.origin}/api/notes`)).status()).toBe(401);
    await other.getByLabel("Recovery key", { exact: true }).fill(recovery);
    await other.getByRole("button", { name: "Unlock notes", exact: true }).click();
    await expect(other.getByRole("textbox", { name: "Note", exact: true })).toHaveText(/continued/);
  } finally {
    await second.close();
  }
});

test("protects competing edits and remembers independent enrolled browser profiles", async ({
  page,
  browser,
  sealed,
}) => {
  await page.goto(sealed.origin);
  const recovery = await page.getByLabel("Recovery key", { exact: true }).inputValue();
  await page.getByLabel("I saved the recovery key").check();
  await page.getByRole("button", { name: "Start writing" }).click();
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.fill("# Shared profiles\n\nOriginal text");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const address = page.url();
  const name = new URL(address).searchParams.get("note");
  const second = await browser.newContext();
  try {
    const peer = await second.newPage();
    await peer.goto(address);
    expect(
      (
        await peer.request.get(`${sealed.origin}/api/notes/${name}`, {
          headers: { "if-none-match": "*" },
        })
      ).status(),
    ).toBe(401);
    await peer.getByLabel("Recovery key", { exact: true }).fill(recovery);
    await peer.getByRole("button", { name: "Unlock notes", exact: true }).click();
    const peerEditor = peer.getByRole("textbox", { name: "Note", exact: true });
    await expect(peerEditor).toHaveText(/Original text/);
    await editor.fill("# Shared profiles\n\nFirst device update");
    await expect(page.locator("#save-status")).toHaveText("Saved");
    await peer.bringToFront();
    await peer.evaluate(() => window.dispatchEvent(new Event("focus")));
    await expect(peerEditor).toHaveText(/First device update/);

    let release: () => void = () => {};
    let arrived: () => void = () => {};
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    const seen = new Promise<void>((resolve) => {
      arrived = resolve;
    });
    await peer.route("**/api/notes/*", async (route) => {
      if (route.request().method() !== "PUT") return route.continue();
      arrived();
      await held;
      await route.continue();
    });
    await peerEditor.fill("# Shared profiles\n\nKeep the competing draft");
    await seen;
    await editor.fill("# Shared profiles\n\nThe acknowledged winner");
    await expect(page.locator("#save-status")).toHaveText("Saved");
    release();
    await expect(peer.locator("#save-status")).toHaveText("Conflict");
    await expect(peerEditor).toHaveText(/Keep the competing draft/);
    expect(
      (await (await page.request.get(`${sealed.origin}/api/notes/${name}`)).json()).body,
    ).toContain("The acknowledged winner");
    await peer.getByRole("button", { name: "Save a copy", exact: true }).click();
    await expect(peer.locator("#save-status")).toHaveText("Saved");
    expect(peer.url()).not.toBe(address);
    await Promise.all([
      editor.fill("# Shared profiles\n\nIndependent writing on the first device"),
      peerEditor.fill("# Recovered copy\n\nIndependent writing on the second device"),
    ]);
    await expect(page.locator("#save-status")).toHaveText("Saved");
    await expect(peer.locator("#save-status")).toHaveText("Saved");
    await sealed.restart();
    await Promise.all([page.reload(), peer.reload()]);
    await expect(editor).toHaveText(/Independent writing on the first device/);
    await expect(peerEditor).toHaveText(/Independent writing on the second device/);
    await expect(page.getByRole("heading", { name: "Unlock notes" })).toHaveCount(0);
    await expect(peer.getByRole("heading", { name: "Unlock notes" })).toHaveCount(0);
  } finally {
    await second.close();
  }
});

test("remembers the device key after both browser and server processes restart", async ({
  playwright,
  browserName,
  sealed,
}) => {
  const profile = `../target/sealed-device-${browserName}-${randomUUID()}`;
  const engine = playwright[browserName];
  let browser = await engine.launchPersistentContext(profile, { headless: true });
  let address = "";
  try {
    const page = await browser.newPage();
    await page.goto(sealed.origin);
    await page.getByLabel("I saved the recovery key").check();
    await page.getByRole("button", { name: "Start writing" }).click();
    await page
      .getByRole("textbox", { name: "Note", exact: true })
      .fill("# Remembered device\n\nOriginal private text");
    await expect(page.locator("#save-status")).toHaveText("Saved");
    address = page.url();
  } finally {
    await browser.close();
  }
  await sealed.restart();
  browser = await engine.launchPersistentContext(profile, { headless: true });
  try {
    const page = await browser.newPage();
    await page.goto(address);
    await expect(page.getByRole("textbox", { name: "Note", exact: true })).toHaveText(
      /Original private text/,
    );
    await expect(page.getByRole("heading", { name: "Unlock notes" })).toHaveCount(0);
  } finally {
    await browser.close();
  }
});

test("keeps setup retryable when persistent device storage is unavailable", async ({
  page,
  sealed,
}) => {
  await page.addInitScript(() =>
    Object.defineProperty(globalThis, "indexedDB", {
      get() {
        throw new Error("Device storage unavailable");
      },
    }),
  );
  await page.goto(sealed.origin);
  await page.getByLabel("I saved the recovery key").check();
  await page.getByRole("button", { name: "Start writing" }).click();
  await expect(page.locator("#vault-entry-notice")).toContainText("Device storage unavailable");
  await expect(page.getByRole("button", { name: "Start writing" })).toBeEnabled();
  expect((await page.request.get(`${sealed.origin}/api/vault`)).ok()).toBe(true);
  expect((await (await page.request.get(`${sealed.origin}/api/vault`)).json()).manifest).toBeNull();
});

test("keeps original files, history, and Trash usable inside encrypted storage", async ({
  page,
  sealed,
}) => {
  await page.goto(sealed.origin);
  await page.getByLabel("I saved the recovery key").check();
  await page.getByRole("button", { name: "Start writing" }).click();
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await expect(editor).toBeEditable();
  await editor.fill("# Original files\n\nPrivate text");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const bytes = Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j3ioAAAAASUVORK5CYII=",
    "base64",
  );
  await attachToNote(page, { name: "private-image.png", mimeType: "image/png", buffer: bytes });
  await editor.press("End");
  await editor.press("Enter");
  await editor.press("Enter");
  await editor.pressSequentially("Below the image");
  const image = page.locator("#live-editor img");
  await expect(image).toBeVisible();
  const path = await image.getAttribute("src");
  if (!path) throw new Error("The original file link is missing.");
  const response = await page.request.get(new URL(path, sealed.origin).href, {
    headers: { range: "bytes=0-7" },
  });
  expect(response.status()).toBe(206);
  expect(await response.body()).toEqual(bytes.subarray(0, 8));
  await page.getByRole("button", { name: "More", exact: true }).click();
  page.once("dialog", (dialog) => {
    void dialog.accept();
  });
  await page.getByRole("button", { name: "Delete", exact: true }).click();
  await expect(page.locator("#body")).not.toHaveValue(/Private text/);
  const trash = await openTrash(page);
  await expect(trash).toContainText("Original files");
  await trash.getByRole("option").first().click();
  await trash.getByRole("button", { name: "Restore", exact: true }).click();
  await expect(editor).toHaveText(/Private text/);
  await expect(page.locator("#live-editor img")).toBeVisible();
});
