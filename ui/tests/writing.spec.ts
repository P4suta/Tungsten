import { accessibilityViolations } from "./accessibility.js";
import { expect, markdownValue, pressShortcut, test } from "./editor.js";
import { newEmptyNote, openNavigation, openTrash, showPaper } from "./interface.js";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByLabel("Note", { exact: true })).toBeEditable();
  await newEmptyNote(page);
});

test("allows clearing the dated title without leaving a writing prompt", async ({ page }, info) => {
  const note = page.getByLabel("Note", { exact: true });
  await expect(note).not.toHaveAttribute("placeholder");
  await expect(note).toHaveMarkdown("");
  await expect(page.locator("#save-status")).toHaveText(/^(Ready|Saved)$/);
  expect(await page.evaluate(() => document.documentElement.scrollHeight <= innerHeight + 1)).toBe(
    true,
  );
  await page.screenshot({ path: `../target/screenshots/writing-empty-${info.project.name}.png` });
});

test("keeps the last writing line centered through wrapping and narrow layouts", async ({
  page,
}) => {
  const note = page.getByLabel("Note", { exact: true });
  for (const viewport of [
    { width: 1280, height: 800 },
    { width: 390, height: 700 },
  ]) {
    await page.setViewportSize(viewport);
    for (const text of ["A line\n".repeat(45), "長い文章を続けて書く。".repeat(130)]) {
      await pressShortcut(note, "a");
      await page.keyboard.insertText(text);
      await expect(note).toHaveMarkdown(text);
      await note.pressSequentially("typing");
      await expect(note).toHaveMarkdown(`${text}typing`);
      await expect
        .poll(() =>
          note.evaluate((input) => {
            const selection = window.getSelection();
            if (!selection?.focusNode) return { within: false };
            const range = document.createRange();
            range.setStart(selection.focusNode, selection.focusOffset);
            range.collapse(true);
            const caret = range.getBoundingClientRect().top;
            const line = Number.parseFloat(getComputedStyle(input).lineHeight);
            const paper = document.querySelector("#paper-scroll")?.getBoundingClientRect();
            if (!paper) return { within: false };
            const center = (paper.top + paper.bottom) / 2;
            return {
              within: Math.abs(caret - center) < line,
              caret,
              center,
              line,
              scroll: scrollY,
              height: document.documentElement.scrollHeight,
              spacer: document.querySelector("#typing-space")?.getBoundingClientRect().height,
              editor: input.getBoundingClientRect().toJSON(),
            };
          }),
        )
        .toEqual(expect.objectContaining({ within: true }));
    }
  }
});

test("filters slash commands and inserts a block with the keyboard", async ({ page }) => {
  const note = page.getByLabel("Note", { exact: true });
  await note.pressSequentially("/heading");
  const menu = page.getByRole("listbox", { name: "Insert into note", exact: true });
  await expect(menu).toBeVisible();
  expect(await accessibilityViolations(page)).toEqual([]);
  await note.press("ArrowDown");
  await note.press("Enter");
  await expect(note).toHaveMarkdown("## ");
  await expect(menu).not.toBeVisible();
  await pressShortcut(note, "z");
  await expect(note).toHaveMarkdown("/heading");
  await note.press("Escape");
  await expect(menu).not.toBeVisible();
  await expect(note).toHaveMarkdown("/heading");
  await note.fill("```\n/image");
  await note.pressSequentially(" ");
  await expect(menu).not.toBeVisible();
});

test("inserts an original image through a slash file picker", async ({ page, request }) => {
  const note = page.getByLabel("Note", { exact: true });
  await note.pressSequentially("/file");
  const choosing = page.waitForEvent("filechooser");
  await note.press("Enter");
  const chooser = await choosing;
  expect(await chooser.element().getAttribute("accept")).toBe("");
  const bytes = Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jVfoAAAAASUVORK5CYII=",
    "base64",
  );
  await chooser.setFiles({ name: "original.png", mimeType: "image/png", buffer: bytes });
  await expect(note).toHaveMarkdown(/^!\[original\.png\]\(.+\)$/);
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const id = /\.attachments\/([a-f0-9]{32}\.png)/.exec(await markdownValue(note))?.[1];
  expect(id).toBeDefined();
  expect(await (await request.get(`/.attachments/${id}`)).body()).toEqual(bytes);
});

test("keeps the draft and command when a slash upload fails", async ({ page }) => {
  await page.route("**/api/attachments", (route) =>
    route.request().method() === "POST" ? route.abort() : route.continue(),
  );
  const note = page.getByLabel("Note", { exact: true });
  await note.pressSequentially("/file");
  const choosing = page.waitForEvent("filechooser");
  await note.press("Enter");
  await (await choosing).setFiles({
    name: "kept.txt",
    mimeType: "text/plain",
    buffer: Buffer.from("original"),
  });
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(note).toHaveMarkdown("/file");
});

test("browses Trash beside the current draft and accepts its previous URL", async ({ page }) => {
  const note = page.getByLabel("Note", { exact: true });
  await note.fill("# Current draft\n\nKeep writing here.\n");
  const settings = await openTrash(page);
  const address = page.url();
  await expect(settings.getByRole("button", { name: "Empty trash", exact: true })).toBeVisible();
  await expect(settings.locator("details")).toHaveCount(0);
  const close = settings.getByRole("button", { name: "Back to notes", exact: true });
  await expect(close.locator("svg[aria-hidden=true]")).toBeVisible();
  const positions = await close.evaluate((button) => {
    const bounds = button.getBoundingClientRect();
    const parent = button.parentElement?.getBoundingClientRect();
    return parent !== undefined && bounds.top < parent.top + 16 && bounds.right > parent.right - 16;
  });
  expect(positions).toBe(true);
  await pressShortcut(page, "Enter");
  await expect(settings).toBeVisible();
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await close.click();
  await showPaper(page);
  expect(page.url()).toBe(address);
  await expect(note).toBeVisible();
  await expect(note).toHaveMarkdown("# Current draft\n\nKeep writing here.\n");
  await page.goto("/trash");
  await expect(settings).toBeVisible();
  expect(new URL(page.url()).pathname).toBe("/");
});

test("uses centered typing without a configuration switch", async ({ page }) => {
  await expect(page.locator("#settings-page")).toHaveCount(0);
  const note = page.getByLabel("Note", { exact: true });
  await note.fill("A line\n".repeat(45));
  await pressShortcut(note, "End");
  await note.pressSequentially("typing");
  await expect
    .poll(() =>
      page.locator("#typing-space").evaluate((space) => space.getBoundingClientRect().height),
    )
    .toBeGreaterThan(0);
});

test("keeps a slash command when the file picker is canceled", async ({ page }) => {
  const note = page.getByLabel("Note", { exact: true });
  await note.pressSequentially("/file");
  const choosing = page.waitForEvent("filechooser");
  await note.press("Enter");
  await (await choosing).setFiles([]);
  await expect(note).toHaveMarkdown("/file");
  await expect(note).toBeFocused();
});

test("keeps text typed while a slash upload is pending", async ({ page }) => {
  let release: () => void = () => {};
  let arrived: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const seen = new Promise<void>((resolve) => {
    arrived = resolve;
  });
  await page.route("**/api/attachments", async (route) => {
    if (route.request().method() !== "POST") return route.continue();
    const response = await route.fetch();
    arrived();
    await held;
    await route.fulfill({ response });
  });
  const note = page.getByLabel("Note", { exact: true });
  await note.pressSequentially("/file");
  const choosing = page.waitForEvent("filechooser");
  await note.press("Enter");
  await (await choosing).setFiles({
    name: "pending.txt",
    mimeType: "text/plain",
    buffer: Buffer.from("original"),
  });
  await seen;
  await note.fill("Keep the text written during upload.");
  release();
  await expect(note).toHaveMarkdown(/Keep the text written during upload\.[\s\S]*\.attachments\//);
});

test("keeps standalone upload and file management outside the writing flow", async ({ page }) => {
  await openNavigation(page);
  await expect(page.locator("#sidebar")).not.toContainText("Upload files");
  await expect(page.locator("#files-page, #attach-files, #import-files")).toHaveCount(0);
  expect((await page.request.get("/files")).status()).toBe(404);
});
