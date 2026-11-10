import type { APIRequestContext, Page } from "@playwright/test";
import { accessibilityViolations } from "./accessibility.js";
import { expect, pressShortcut, test } from "./editor.js";
import {
  closeNavigation,
  exploreNotes,
  nameNote,
  newNote,
  noteAction,
  openNavigation,
  showPaper,
} from "./interface.js";

async function showNotes(page: Page): Promise<void> {
  await exploreNotes(page);
}

async function seedNotes(request: APIRequestContext, notes: Record<string, string>): Promise<void> {
  for (const [name, body] of Object.entries(notes))
    expect((await request.post("/api/notes", { data: { name, body } })).ok()).toBe(true);
}

async function seedDeletedDraft(
  page: Page,
  request: APIRequestContext,
  body: string,
): Promise<void> {
  await seedNotes(request, {
    "Survivor.md": "# Survivor\n\nKeep writing.\n",
    "Discarded.md": "# Discarded\n\nOriginal writing.\n",
  });
  const saved: { revision: string; body: string } = await (
    await request.get("/api/notes/Discarded.md")
  ).json();
  expect(
    (
      await request.delete("/api/notes/Discarded.md", {
        headers: { "content-type": "application/json", "if-match": saved.revision },
      })
    ).ok(),
  ).toBe(true);
  await page.evaluate(
    (draft) => {
      localStorage.setItem("tungsten.draft.deleted-fixture", JSON.stringify(draft));
      localStorage.setItem("tungsten.last", draft.name);
    },
    {
      name: "Discarded.md",
      body,
      savedBody: saved.body,
      revision: saved.revision,
      title: "Discarded",
      candidate: "Discarded.md",
      attempted: false,
      updated: Date.now(),
    },
  );
}

test.beforeEach(async ({ page, request }) => {
  const response = await request.get("/api/notes");
  const notes: { name: string }[] = await response.json();
  for (const note of notes) {
    const url = `/api/notes/${encodeURIComponent(note.name)}`;
    const saved = await request.get(url);
    await request.delete(url, {
      headers: { "content-type": "application/json", "if-match": saved.headers().etag ?? "" },
    });
  }
  page.on("dialog", (dialog) => {
    void dialog.accept();
  });
  await page.goto("/");
});

test("writes plain text, searches, renames, and deletes a note", async ({ page, request }) => {
  await nameNote(page, "Shopping");
  await page
    .getByLabel("Note", { exact: true })
    .fill("牛乳\n# Just text\n<script>alert('text')</script>");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const saved = await request.get("/api/notes/Shopping.md");
  expect((await saved.json()).body).toContain("<script>");
  await showNotes(page);
  await page.getByLabel("Search notes").fill("牛乳");
  await page.getByRole("button", { name: "Just text", exact: true }).click();
  await nameNote(page, "Groceries", true);
  await expect(page.locator("#save-status")).toHaveText("Saved");
  expect((await (await request.get("/api/notes/Shopping.md")).json()).body).toContain(
    "# Groceries",
  );
  await noteAction(page, "Delete");
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown(/^# \d{4}-/);
  expect((await request.get("/api/notes")).ok()).toBeTruthy();
  expect(await (await request.get("/api/notes")).json()).toEqual([]);
  await expect(page).toHaveURL("/");
  await page.reload();
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown(/^# \d{4}-/);
  await expect(page.getByRole("alert")).toBeHidden();
});

test("returns to the preceding note after deletion and replaces the deleted address", async ({
  page,
  request,
}, info) => {
  await seedNotes(request, {
    "Parent.md": "# Parent\n\n[Child](Child.md)\n\nEnd\n",
    "Child.md": "# Child\n\nRemove this note.\n",
    "Unrelated.md": "# Unrelated\n\nA different topic.\n",
  });
  await page.goto("/?note=Parent.md");
  const editor = page.getByLabel("Note", { exact: true });
  await pressShortcut(editor, "End");
  await page.locator("#live-editor").getByRole("link", { name: "Child", exact: true }).click();
  await expect(editor).toHaveMarkdown(/^# Child\n/);
  await noteAction(page, "Delete");
  await expect(editor).toHaveMarkdown(/^# Parent\n/);
  await expect(page).toHaveURL(/\?note=Parent.md$/);
  await expect(
    (await page.evaluate(() => document.body.dataset.layout === "wide"))
      ? page.locator("#close-context")
      : editor,
  ).toBeFocused();
  await expect(page.getByRole("dialog", { name: "More", exact: true })).toHaveCount(0);
  expect((await request.get("/api/notes/Child.md")).status()).toBe(404);
  expect(await accessibilityViolations(page)).toEqual([]);
  await page.screenshot({ path: `../target/screenshots/after-deletion-${info.project.name}.png` });
  await page.goBack();
  await expect(page).toHaveURL(/\?note=Parent.md$/);
  await expect(page.getByRole("alert")).toBeHidden();
  await page.reload();
  await expect(editor).toHaveMarkdown(/^# Parent\n/);
});

test("preserves navigation opened while deletion is pending", async ({ page, request }) => {
  await seedNotes(request, { "Discarded.md": "# Discarded", "Survivor.md": "# Survivor" });
  await page.goto("/?note=Discarded.md");
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown("# Discarded");
  let release: (() => void) | undefined;
  let arrived: (() => void) | undefined;
  const pending = new Promise<void>((resolve) => {
    release = resolve;
  });
  const requested = new Promise<void>((resolve) => {
    arrived = resolve;
  });
  await page.route("**/api/notes/Discarded.md", async (route) => {
    if (route.request().method() === "DELETE") {
      arrived?.();
      await pending;
    }
    await route.continue();
  });
  try {
    await noteAction(page, "Delete");
    await requested;
    const menu = page.getByRole("complementary", { name: "Navigation", exact: true });
    await closeNavigation(page);
    await expect(menu).not.toBeVisible();
    await openNavigation(page);
    await expect(menu).toBeVisible();
    release?.();
    await expect(page.locator("#body")).toHaveValue("# Survivor");
    await expect(menu).toBeVisible();
    await expect(menu.getByRole("button", { name: "Close navigation", exact: true })).toBeFocused();
  } finally {
    release?.();
  }
});

test("opens a surviving note after deletion without using the previous search filter", async ({
  page,
  request,
}) => {
  await seedNotes(request, {
    "Survivor.md": "# Survivor\n\nKeep writing.\n",
    "Discarded.md": "# Discarded\n\nRemove this note.\n",
  });
  await page.goto("/?note=Discarded.md");
  await showNotes(page);
  await page.getByLabel("Search notes", { exact: true }).fill("Discarded");
  await page.locator("#notes").getByRole("button", { name: "Discarded", exact: true }).click();
  await noteAction(page, "Delete");
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown(/^# Survivor\n/);
  await expect(page).toHaveURL(/\?note=Survivor.md$/);
  await expect(
    (await page.evaluate(() => document.body.dataset.layout === "wide"))
      ? page.locator("#close-context")
      : page.getByLabel("Note", { exact: true }),
  ).toBeFocused();
});

test("skips a previously visited note that has also moved to trash", async ({ page, request }) => {
  await seedNotes(request, {
    "Parent.md": "# Parent\n\n[Child](Child.md)\n\nEnd\n",
    "Child.md": "# Child\n\nRemove this note.\n",
    "Survivor.md": "# Survivor\n\nKeep writing.\n",
  });
  await page.goto("/?note=Parent.md");
  const editor = page.getByLabel("Note", { exact: true });
  await pressShortcut(editor, "End");
  await page.locator("#live-editor").getByRole("link", { name: "Child", exact: true }).click();
  await expect(editor).toHaveMarkdown(/^# Child\n/);
  const saved: { revision: string } = await (await request.get("/api/notes/Parent.md")).json();
  expect(
    (
      await request.delete("/api/notes/Parent.md", {
        headers: { "content-type": "application/json", "if-match": saved.revision },
      })
    ).ok(),
  ).toBe(true);
  await noteAction(page, "Delete");
  await expect(editor).toHaveMarkdown(/^# Survivor\n/);
  await expect(page.getByRole("alert")).toBeHidden();
});

test("leaves a note already moved to trash instead of keeping it in the editor", async ({
  page,
  request,
}) => {
  await seedNotes(request, {
    "Survivor.md": "# Survivor\n\nKeep writing.\n",
    "Discarded.md": "# Discarded\n\nRemove this note.\n",
  });
  await page.goto("/?note=Discarded.md");
  const editor = page.getByLabel("Note", { exact: true });
  await expect(editor).toHaveMarkdown(/^# Discarded\n/);
  const saved: { revision: string } = await (await request.get("/api/notes/Discarded.md")).json();
  expect(
    (
      await request.delete("/api/notes/Discarded.md", {
        headers: { "content-type": "application/json", "if-match": saved.revision },
      })
    ).ok(),
  ).toBe(true);
  await noteAction(page, "Delete");
  await expect(editor).toHaveMarkdown(/^# Survivor\n/);
  await expect(page).toHaveURL(/\?note=Survivor.md$/);
  await expect(page.getByRole("alert")).toBeHidden();
});

test("preserves the active note when deletion fails", async ({ page, request }) => {
  await seedNotes(request, { "Kept.md": "# Kept\n\nOriginal writing.\n" });
  await page.goto("/?note=Kept.md");
  await page.route("**/api/notes/Kept.md", (route) =>
    route.request().method() === "DELETE"
      ? route.fulfill({ status: 503, json: { error: "Deletion is unavailable." } })
      : route.continue(),
  );
  await noteAction(page, "Delete");
  await expect(page.getByRole("dialog", { name: "More", exact: true })).toHaveCount(0);
  await expect(page.getByRole("alert")).toHaveText("Deletion is unavailable.");
  await showPaper(page);
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown(
    "# Kept\n\nOriginal writing.\n",
  );
  await expect(page).toHaveURL(/\?note=Kept.md$/);
  expect((await request.get("/api/notes/Kept.md")).ok()).toBe(true);
});

test("does not replace a new draft when loading the note after deletion finishes late", async ({
  page,
  request,
}) => {
  await seedNotes(request, {
    "Survivor.md": "# Survivor\n\nKeep writing.\n",
    "Discarded.md": "# Discarded\n\nRemove this note.\n",
  });
  await page.goto("/?note=Discarded.md");
  let release: () => void = () => {};
  let arrived: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const seen = new Promise<void>((resolve) => {
    arrived = resolve;
  });
  await page.route("**/api/notes/Survivor.md", async (route) => {
    const response = await route.fetch();
    arrived();
    await held;
    await route.fulfill({ response });
  });
  await noteAction(page, "Delete");
  await seen;
  await expect(page.locator("#body")).not.toHaveValue(/Discarded/);
  await expect(page).not.toHaveURL(/note=Discarded.md/);
  await newNote(page);
  const editor = page.getByLabel("Note", { exact: true });
  await editor.fill("# New draft\n\nKeep my latest writing.\n");
  release();
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await expect(editor).toHaveMarkdown("# New draft\n\nKeep my latest writing.\n");
  await expect(page).not.toHaveURL(/note=(Discarded|Survivor).md/);
});

test("preserves a pending browser navigation when deletion finishes late", async ({
  page,
  request,
}) => {
  await seedNotes(request, {
    "Parent.md": "# Parent\n\n[Child](Child.md)\n\nEnd\n",
    "Child.md": "# Child\n\nRemove this note.\n",
  });
  await page.goto("/?note=Parent.md");
  const editor = page.getByLabel("Note", { exact: true });
  await pressShortcut(editor, "End");
  await page.locator("#live-editor").getByRole("link", { name: "Child", exact: true }).click();
  await expect(editor).toHaveMarkdown(/^# Child\n/);
  let deleted: () => void = () => {};
  let releaseDeletion: () => void = () => {};
  let opening: () => void = () => {};
  let releaseOpening: () => void = () => {};
  const deletionSeen = new Promise<void>((resolve) => {
    deleted = resolve;
  });
  const deletionHeld = new Promise<void>((resolve) => {
    releaseDeletion = resolve;
  });
  const openingSeen = new Promise<void>((resolve) => {
    opening = resolve;
  });
  const openingHeld = new Promise<void>((resolve) => {
    releaseOpening = resolve;
  });
  await page.route("**/api/notes/Child.md", async (route) => {
    if (route.request().method() !== "DELETE") return route.continue();
    const response = await route.fetch();
    deleted();
    await deletionHeld;
    await route.fulfill({ response });
  });
  await page.route("**/api/notes/Parent.md", async (route) => {
    const response = await route.fetch();
    opening();
    await openingHeld;
    await route.fulfill({ response });
  });
  await noteAction(page, "Delete");
  await deletionSeen;
  await page.keyboard.press("Escape");
  await page.goBack();
  await openingSeen;
  releaseDeletion();
  await expect(page.locator("#body")).not.toHaveValue(/Child/);
  await expect(page).toHaveURL(/\?note=Parent.md$/);
  await expect(page.locator("#save-status")).toHaveText("Loading");
  releaseOpening();
  await expect(editor).toHaveMarkdown(/^# Parent\n/);
  await expect(page.getByRole("alert")).toBeHidden();
});

test("keeps deleted content out of the editor when the replacement cannot load", async ({
  page,
  request,
}) => {
  await seedNotes(request, {
    "Survivor.md": "# Survivor\n\nKeep writing.\n",
    "Discarded.md": "# Discarded\n\nRemove this note.\n",
  });
  await page.goto("/?note=Discarded.md");
  await page.route("**/api/notes/Survivor.md", (route) =>
    route.fulfill({ status: 503, json: { error: "The next note is unavailable." } }),
  );
  await noteAction(page, "Delete");
  await expect(page.getByRole("alert")).toContainText("The next note is unavailable.");
  await showPaper(page);
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown(/^# \d{4}-/);
  await expect(page).toHaveURL("/");
  expect((await request.get("/api/notes/Discarded.md")).status()).toBe(404);
  await page.unroute("**/api/notes/Survivor.md");
  await page.reload();
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown(/^# Survivor\n/);
});

test("does not recover a deleted note when the collection refresh fails", async ({
  page,
  request,
}) => {
  await seedNotes(request, {
    "Survivor.md": "# Survivor\n\nKeep writing.\n",
    "Discarded.md": "# Discarded\n\nRemove this note.\n",
  });
  await page.goto("/?note=Discarded.md");
  const collection = /\/api\/notes\?paged=true&limit=24(?:&|$)/;
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown(/^# Discarded\n/);
  await page.route(collection, (route) =>
    route.fulfill({ status: 404, json: { error: "The collection is unavailable." } }),
  );
  await noteAction(page, "Delete");
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown(/^# Survivor\n/);
  await expect(page.locator("#list-status")).toHaveText("The collection is unavailable.");
  await page.unroute(collection);
  await page.goto("/");
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown(/^# Survivor\n/);
  await expect(page.getByRole("alert")).toBeHidden();
});

test("skips a clean browser draft for a deleted note", async ({ page, request }) => {
  await seedDeletedDraft(page, request, "# Discarded\n\nOriginal writing.\n");
  await page.goto("/");
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown(/^# Survivor\n/);
  await expect(page).toHaveURL(/\?note=Survivor.md$/);
  await expect(page.getByRole("alert")).toBeHidden();
  expect(
    await page.evaluate(() => localStorage.getItem("tungsten.draft.deleted-fixture")),
  ).toBeNull();
});

test("preserves an unsaved browser draft for a deleted note", async ({ page, request }) => {
  const writing = "# Discarded\n\nUnsaved writing.\n";
  await seedDeletedDraft(page, request, writing);
  await page.goto("/");
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown(writing);
  await expect(page.locator("#save-status")).toHaveText("Conflict");
  await expect(page.getByRole("button", { name: "Save a copy", exact: true })).toBeVisible();
  expect((await request.get("/api/notes/Discarded.md")).status()).toBe(404);
});

test("recovers an unsaved draft after a failed save and reload", async ({ page }) => {
  await page.getByLabel("Note", { exact: true }).fill("saved text");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await page.route("**/api/notes/*", async (route) => {
    if (route.request().method() === "PUT") await route.abort();
    else await route.continue();
  });
  await page.getByLabel("Note", { exact: true }).fill("draft after failure");
  await expect(page.locator("#save-status")).toHaveText("Save failed");
  await page.reload();
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown("draft after failure");
  await expect(page.getByRole("button", { name: "Retry", exact: true })).toBeVisible();
  await page.unroute("**/api/notes/*");
  await page.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(page.locator("#save-status")).toHaveText("Saved");
});

test("reports an unavailable linked note while keeping the blank page editable", async ({
  page,
}) => {
  await page.goto("/?note=Missing.md");
  await expect(page.getByLabel("Note", { exact: true })).toBeEditable();
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown(/^# \d{4}-/);
});

test("keeps edits made while an earlier save response is delayed", async ({ page, request }) => {
  await page.getByLabel("Note", { exact: true }).fill("initial");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  let release: () => void = () => {};
  let arrived: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const seen = new Promise<void>((resolve) => {
    arrived = resolve;
  });
  let first = true;
  await page.route("**/api/notes/*", async (route) => {
    if (route.request().method() === "PUT" && first) {
      first = false;
      const response = await route.fetch();
      arrived();
      await held;
      await route.fulfill({ response });
    } else await route.continue();
  });
  await page.getByLabel("Note", { exact: true }).fill("first edit");
  await seen;
  await page.getByLabel("Note", { exact: true }).fill("latest edit");
  await expect(page.locator("#save-status")).toHaveText("Saving");
  release();
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown("latest edit");
  const notes: { name: string }[] = await (await request.get("/api/notes")).json();
  const note = notes[0];
  expect(note).toBeDefined();
  const saved = await request.get(`/api/notes/${encodeURIComponent(note?.name ?? "")}`);
  expect((await saved.json()).body).toBe("latest edit");
});

test("waits for composition to end before saving Japanese input", async ({ page, request }) => {
  await page.clock.install();
  const textarea = page.getByLabel("Note", { exact: true });
  await expect(textarea).toBeEditable();
  await page.evaluate(() => {
    document.dispatchEvent(new Event("compositionstart", { bubbles: true }));
  });
  await page.locator("#body").evaluate((input: HTMLTextAreaElement) => {
    input.value = "日本語の変換中";
    input.dispatchEvent(
      new InputEvent("input", {
        bubbles: true,
        inputType: "insertCompositionText",
        data: input.value,
        isComposing: true,
      }),
    );
  });
  await page.clock.fastForward(1000);
  expect(await (await request.get("/api/notes")).json()).toEqual([]);
  await page.evaluate(() => {
    document.dispatchEvent(new Event("compositionend", { bubbles: true }));
  });
  await page.clock.fastForward(1000);
  await expect(page.locator("#save-status")).toHaveText("Saved");
});

test("preserves a stale tab's draft and saves a separate copy", async ({
  page,
  context,
  request,
}) => {
  await nameNote(page, "Shared");
  await page.getByLabel("Note", { exact: true }).fill("initial");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const other = await context.newPage();
  await other.goto("/");
  await expect(other.getByLabel("Note", { exact: true })).toHaveMarkdown("initial");
  let release: () => void = () => {};
  let arrived: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const seen = new Promise<void>((resolve) => {
    arrived = resolve;
  });
  await other.route("**/api/notes/Shared.md", async (route) => {
    if (route.request().method() !== "PUT") return route.continue();
    arrived();
    await held;
    await route.continue();
  });
  await other.getByLabel("Note", { exact: true }).fill("second tab draft");
  await seen;
  await page.getByLabel("Note", { exact: true }).fill("first tab wins");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  release();
  await expect(other.locator("#save-status")).toHaveText("Conflict");
  await expect(other.getByLabel("Note", { exact: true })).toHaveMarkdown("second tab draft");
  await other.getByRole("button", { name: "Save a copy", exact: true }).click();
  await expect(other.locator("#save-status")).toHaveText("Saved");
  expect((await (await request.get("/api/notes/Shared.md")).json()).body).toBe("first tab wins");
  expect((await (await request.get("/api/notes")).json()).length).toBe(2);
  await other.close();
});

test("recovers a cleared draft when creation was not acknowledged", async ({ page }) => {
  let release: () => void = () => {};
  let arrived: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const seen = new Promise<void>((resolve) => {
    arrived = resolve;
  });
  await page.route("**/api/notes", async (route) => {
    if (route.request().method() !== "POST") {
      await route.continue();
      return;
    }
    const response = await route.fetch();
    arrived();
    await held;
    await route.fulfill({ response });
  });
  await page.getByLabel("Note", { exact: true }).fill("text to clear");
  await seen;
  await page.getByLabel("Note", { exact: true }).fill("");
  await page.reload();
  release();
  await expect(page.getByLabel("Note", { exact: true })).toBeEditable();
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown("");
});

test("keeps a title containing path punctuation as ordinary note content", async ({
  page,
  request,
}) => {
  await page
    .getByLabel("Note", { exact: true })
    .fill("# ../Title / with punctuation\n\nkeep this draft");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const notes: { name: string; title: string }[] = await (await request.get("/api/notes")).json();
  expect(notes[0]?.name).toMatch(/^[a-f0-9]{32}\.md$/);
  expect(notes[0]?.title).toBe("../Title / with punctuation");
});

test("supports narrow layouts, text enlargement, labels, and keyboard access", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 640 });
  await expect(page.getByRole("button", { name: "Navigation", exact: true })).toBeVisible();
  await expect(page.getByLabel("Note", { exact: true })).toBeVisible();
  await page.evaluate(() => {
    const root = document.documentElement;
    root.style.fontSize = `${Number.parseFloat(getComputedStyle(root).fontSize) * 2}px`;
  });
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
  ).toBeTruthy();
  await exploreNotes(page);
  await expect(page.getByLabel("Search notes")).toBeVisible();
  await page
    .getByRole("dialog", { name: "Explore notes", exact: true })
    .getByRole("button", { name: "Close", exact: true })
    .click();
  await newNote(page);
  await expect(page.getByLabel("Note", { exact: true })).toBeFocused();
  await page.evaluate(() => {
    for (const element of document.querySelectorAll("p, label, button, input, textarea")) {
      if (!(element instanceof HTMLElement)) continue;
      element.style.lineHeight = "1.5";
      element.style.letterSpacing = "0.12em";
      element.style.wordSpacing = "0.16em";
      if (element.tagName === "P") element.style.marginBottom = "2em";
    }
  });
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
  ).toBeTruthy();
  await page.setViewportSize({ width: 800, height: 375 });
  await page.getByLabel("Note", { exact: true }).fill("Spacing and landscape input");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
  ).toBeTruthy();
  expect(await accessibilityViolations(page)).toEqual([]);
});

test("explains the JavaScript requirement without allowing unsaved input", async ({
  browser,
  baseURL,
}) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  try {
    const page = await context.newPage();
    await page.goto(baseURL ?? "http://127.0.0.1:18080");
    expect(await page.locator("noscript").textContent()).toBe("Enable JavaScript to edit notes.");
    await expect(page.locator("noscript")).toBeVisible();
    await expect(page.getByLabel("Note", { exact: true })).toBeDisabled();
  } finally {
    await context.close();
  }
});
