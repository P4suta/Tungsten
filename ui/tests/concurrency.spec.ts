import { randomUUID } from "node:crypto";
import type { APIRequestContext, Page } from "@playwright/test";
import { expect, test } from "./editor.js";

interface SavedNote {
  name: string;
  body: string;
  revision: string;
}

async function seed(
  request: APIRequestContext,
  body = "# Shared\n\nOriginal text",
): Promise<SavedNote> {
  const response = await request.post("/api/notes", {
    data: { name: `shared-${randomUUID()}.md`, body },
  });
  expect(response.status()).toBe(201);
  return response.json();
}

async function replace(
  request: APIRequestContext,
  note: SavedNote,
  body: string,
): Promise<SavedNote> {
  const response = await request.put(`/api/notes/${encodeURIComponent(note.name)}`, {
    headers: { "if-match": note.revision },
    data: { body },
  });
  expect(response.status()).toBe(200);
  return response.json();
}

async function foreground(page: Page): Promise<void> {
  await page.bringToFront();
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
}

test("refreshes a clean note on foreground and revalidates unchanged content", async ({
  page,
  request,
}) => {
  const note = await seed(request);
  await page.goto(`/?note=${encodeURIComponent(note.name)}`);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await expect(editor).toHaveMarkdown(note.body);
  const changed = "# Shared\n\nWritten on another device";
  const remote = await replace(request, note, changed);
  await foreground(page);
  await expect(editor).toHaveMarkdown(changed);
  const unchanged = page.waitForRequest(
    (request) =>
      request.method() === "GET" && request.url().endsWith(encodeURIComponent(note.name)),
  );
  await foreground(page);
  const conditional = await unchanged;
  expect(conditional.headers()["if-none-match"]).toBe(remote.revision);
  const response = await conditional.response();
  expect(response).not.toBeNull();
  expect([200, 304]).toContain(response?.status());
  await response?.finished();
  expect((await conditional.sizes()).responseBodySize).toBe(0);
  await expect(editor).toHaveMarkdown(changed);
  await expect(page.locator("#save-status")).toHaveText("Saved");
});

test("does not import a live sibling tab's draft and recovers its own draft on reload", async ({
  page,
  context,
  request,
}) => {
  const note = await seed(request);
  const address = `/?note=${encodeURIComponent(note.name)}`;
  await page.goto(address);
  await page.route("**/api/notes/*", async (route) => {
    if (route.request().method() === "PUT") await route.abort();
    else await route.continue();
  });
  const draft = "# Shared\n\nKeep this tab's unsaved writing";
  await page.getByRole("textbox", { name: "Note", exact: true }).fill(draft);
  await expect(page.locator("#save-status")).toHaveText("Save failed");
  const other = await context.newPage();
  try {
    await other.goto(address);
    await expect(other.getByRole("textbox", { name: "Note", exact: true })).toHaveMarkdown(
      note.body,
    );
    await page.reload();
    await expect(page.getByRole("textbox", { name: "Note", exact: true })).toHaveMarkdown(draft);
    await expect(page.getByRole("button", { name: "Retry", exact: true })).toBeVisible();
  } finally {
    await other.close();
  }
});

test("checks the current note periodically without polling hidden pages", async ({
  page,
  request,
}) => {
  const note = await seed(request);
  await page.clock.install();
  await page.goto(`/?note=${encodeURIComponent(note.name)}`);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await expect(editor).toHaveMarkdown(note.body);
  const changed = "# Shared\n\nAnother device saved this";
  await replace(request, note, changed);
  await page.clock.fastForward(15000);
  await expect(editor).toHaveMarkdown(changed);
  let reads = 0;
  page.on("request", (request) => {
    if (request.url().endsWith(encodeURIComponent(note.name))) reads++;
  });
  await page.evaluate(() => {
    Object.defineProperty(document, "hidden", { configurable: true, value: true });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await page.clock.fastForward(60000);
  expect(reads).toBe(0);
});

test("preserves writing made while a remote refresh is pending", async ({ page, request }) => {
  const note = await seed(request);
  await page.goto(`/?note=${encodeURIComponent(note.name)}`);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await expect(editor).toHaveMarkdown(note.body);
  const remote = "# Shared\n\nRemote writing";
  await replace(request, note, remote);
  let release: () => void = () => {};
  let arrived: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const seen = new Promise<void>((resolve) => {
    arrived = resolve;
  });
  await page.route("**/api/notes/*", async (route) => {
    if (route.request().method() === "PUT") return route.abort();
    if (route.request().headers()["if-none-match"]) {
      const response = await route.fetch();
      arrived();
      await held;
      await route.fulfill({ response });
    } else await route.continue();
  });
  await foreground(page);
  await seen;
  const local = "# Shared\n\nLocal writing must survive";
  await editor.fill(local);
  release();
  await expect(page.locator("#save-status")).toHaveText("Conflict");
  await expect(editor).toHaveMarkdown(local);
  expect((await (await request.get(`/api/notes/${note.name}`)).json()).body).toBe(remote);
});

test("defers a pending refresh until composition ends", async ({ page, request }) => {
  const note = await seed(request);
  await page.goto(`/?note=${note.name}`);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await expect(editor).toHaveMarkdown(note.body);
  const remote = "# Shared\n\nRemote writing during composition";
  await replace(request, note, remote);
  let release: () => void = () => {};
  let arrived: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const seen = new Promise<void>((resolve) => {
    arrived = resolve;
  });
  await page.route("**/api/notes/*", async (route) => {
    if (!route.request().headers()["if-none-match"]) return route.continue();
    const response = await route.fetch();
    arrived();
    await held;
    await route.fulfill({ response });
  });
  await foreground(page);
  await seen;
  await page.evaluate(() =>
    document.dispatchEvent(new Event("compositionstart", { bubbles: true })),
  );
  const returned = page.waitForResponse(
    (response) => response.url().endsWith(note.name) && response.status() === 200,
  );
  release();
  await returned;
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  );
  await expect(editor).toHaveMarkdown(note.body);
  await page.evaluate(() => document.dispatchEvent(new Event("compositionend", { bubbles: true })));
  await foreground(page);
  await expect(editor).toHaveMarkdown(remote);
});

test("acknowledges identical writing saved by another device without a conflict", async ({
  page,
  request,
}) => {
  const note = await seed(request);
  await page.goto(`/?note=${note.name}`);
  await page.route("**/api/notes/*", async (route) => {
    if (route.request().method() === "PUT") return route.abort();
    await route.continue();
  });
  const text = "# Shared\n\nThe same writing on both devices";
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await expect(editor).toHaveMarkdown(note.body);
  await editor.fill(text);
  await expect(page.locator("#save-status")).toHaveText("Save failed");
  await replace(request, note, text);
  await foreground(page);
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await expect(editor).toHaveMarkdown(text);
  await expect(page.getByRole("button", { name: "Save a copy", exact: true })).toBeHidden();
});

test("ignores a delayed refresh after navigating to another note", async ({ page, request }) => {
  const first = await seed(request);
  const second = await seed(request, "# Separate note\n\nKeep this page open");
  await page.goto(`/?note=${first.name}`);
  await expect(page.getByRole("textbox", { name: "Note", exact: true })).toHaveMarkdown(first.body);
  await replace(request, first, "# Shared\n\nAn older response");
  let release: () => void = () => {};
  let arrived: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const seen = new Promise<void>((resolve) => {
    arrived = resolve;
  });
  await page.route(`**/api/notes/${first.name}`, async (route) => {
    const response = await route.fetch();
    arrived();
    await held;
    await route.fulfill({ response });
  });
  await foreground(page);
  await seen;
  await page.evaluate((name) => {
    history.pushState(null, "", `/?note=${encodeURIComponent(name)}`);
    window.dispatchEvent(new PopStateEvent("popstate"));
  }, second.name);
  await expect(page.getByRole("textbox", { name: "Note", exact: true })).toHaveMarkdown(
    second.body,
  );
  release();
  await expect(page.getByRole("textbox", { name: "Note", exact: true })).toHaveMarkdown(
    second.body,
  );
  expect(new URL(page.url()).searchParams.get("note")).toBe(second.name);
});

test("leaves a remotely deleted clean note and protects a deleted dirty note", async ({
  page,
  request,
}) => {
  const survivor = await seed(request, "# Survivor\n\nKeep writing here");
  const deleted = await seed(request);
  await page.goto(`/?note=${deleted.name}`);
  await expect(page.getByRole("textbox", { name: "Note", exact: true })).toHaveMarkdown(
    deleted.body,
  );
  expect(
    (
      await request.delete(`/api/notes/${deleted.name}`, {
        headers: { "if-match": deleted.revision, "content-type": "application/json" },
      })
    ).status(),
  ).toBe(204);
  await foreground(page);
  await expect(page.getByRole("textbox", { name: "Note", exact: true })).toHaveMarkdown(
    survivor.body,
  );
  const dirty = await seed(request, "# Dirty\n\nSaved version");
  await page.goto(`/?note=${dirty.name}`);
  await page.route("**/api/notes/*", async (route) => {
    if (route.request().method() === "PUT") await route.abort();
    else await route.continue();
  });
  const draft = "# Dirty\n\nUnsaved version";
  await page.getByRole("textbox", { name: "Note", exact: true }).fill(draft);
  await expect(page.locator("#save-status")).toHaveText("Save failed");
  expect(
    (
      await request.delete(`/api/notes/${dirty.name}`, {
        headers: { "if-match": dirty.revision, "content-type": "application/json" },
      })
    ).status(),
  ).toBe(204);
  await foreground(page);
  await expect(page.locator("#save-status")).toHaveText("Conflict");
  await expect(page.getByRole("textbox", { name: "Note", exact: true })).toHaveMarkdown(draft);
  await expect(page.getByRole("button", { name: "Reload saved", exact: true })).toBeHidden();
  await expect(page.getByRole("button", { name: "Save a copy", exact: true })).toBeVisible();
});

test("keeps drafts available when ownership coordination is unavailable", async ({
  page,
  context,
  request,
}) => {
  const note = await seed(request);
  await page.addInitScript(() =>
    Object.defineProperty(navigator, "locks", { get: () => undefined }),
  );
  await page.goto(`/?note=${note.name}`);
  await page.route("**/api/notes/*", async (route) => {
    if (route.request().method() === "PUT") await route.abort();
    else await route.continue();
  });
  const draft = "# Shared\n\nUncoordinated writing";
  await page.getByRole("textbox", { name: "Note", exact: true }).fill(draft);
  await expect(page.locator("#save-status")).toHaveText("Save failed");
  const other = await context.newPage();
  try {
    await other.goto(`/?note=${note.name}`);
    await expect(other.getByRole("textbox", { name: "Note", exact: true })).toHaveMarkdown(
      note.body,
    );
    await other.getByRole("link", { name: "Explore notes", exact: true }).click();
    const recover = other.getByRole("button", { name: "Recover Shared", exact: true });
    await expect(recover).toBeVisible();
    await recover.click();
    await other
      .getByRole("dialog", { name: "Explore notes", exact: true })
      .getByRole("button", { name: "Close", exact: true })
      .click();
    await expect(other.getByRole("textbox", { name: "Note", exact: true })).toHaveMarkdown(draft);
  } finally {
    await other.close();
  }
});
