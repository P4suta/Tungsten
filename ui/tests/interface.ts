import type { FileChooser, Locator, Page } from "@playwright/test";
import { clearMarkdown, expect, markdownValue, pressShortcut } from "./editor.js";

export async function openNavigation(page: Page): Promise<Locator> {
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          document.body.dataset.layout ===
          (matchMedia("(min-width: 64rem)").matches ? "wide" : "compact"),
      ),
    )
    .toBe(true);
  const sidebar = page.getByRole("complementary", { name: "Notes", exact: true });
  if (!(await sidebar.isVisible()))
    await page.getByRole("button", { name: "Notes", exact: true }).click();
  await sidebar.getByRole("button", { name: "Close notes", exact: true }).focus();
  return sidebar;
}

export async function closeNavigation(page: Page): Promise<void> {
  const sidebar = page.getByRole("complementary", { name: "Notes", exact: true });
  if (await sidebar.isVisible())
    await sidebar.getByRole("button", { name: "Close notes", exact: true }).click();
}

export async function newNote(page: Page): Promise<void> {
  await page.getByRole("button", { name: "New note", exact: true }).click();
  await expect(page.locator("#paper-scroll")).toBeVisible();
}

export async function exploreNotes(page: Page): Promise<void> {
  await openNavigation(page);
  await page.getByLabel("Search all notes", { exact: true }).focus();
}

export async function openWorkspaces(page: Page): Promise<Locator> {
  const sidebar = await openNavigation(page);
  const browser = sidebar.locator("#workspace-browser");
  if ((await browser.getAttribute("open")) === null) await browser.locator("summary").click();
  return sidebar;
}

export async function noteAction(page: Page, name: string): Promise<void> {
  const sidebar = await openContext(page);
  const action = sidebar.getByRole("button", { name, exact: true });
  await action.focus();
  await action.click();
}

export async function openContext(page: Page): Promise<Locator> {
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          document.body.dataset.layout ===
          (matchMedia("(min-width: 64rem)").matches ? "wide" : "compact"),
      ),
    )
    .toBe(true);
  const sidebar = page.getByRole("dialog", { name: "Note details", exact: true });
  if (!(await sidebar.isVisible()))
    await page.getByRole("button", { name: "Note details", exact: true }).click();
  return sidebar;
}

export async function showPaper(page: Page): Promise<void> {
  if (await page.locator("#note-sidebar").isVisible())
    await page.getByRole("button", { name: "Close note details", exact: true }).click();
  if (!(await page.locator("#paper-scroll").isVisible()))
    await page.getByRole("button", { name: "Back to note", exact: true }).click();
  await expect(page.locator("#paper-scroll")).toBeVisible();
}

export async function nameNote(page: Page, name: string, rename = false): Promise<void> {
  if (rename) {
    const body = page.getByLabel("Note", { exact: true });
    const text = await markdownValue(body);
    await body.fill(
      /^#\s+.+$/m.test(text) ? text.replace(/^#\s+.+$/m, `# ${name}`) : `# ${name}\n\n${text}`,
    );
    return;
  }
  const response = await page.request.post("/api/notes", {
    data: { name: `${name}.md`, body: "" },
  });
  expect(response.ok()).toBeTruthy();
  await page.goto(`/?note=${encodeURIComponent(`${name}.md`)}`);
  await expect(page.getByLabel("Note", { exact: true })).toBeEditable();
}

export async function openTrash(page: Page): Promise<Locator> {
  const navigation = await openNavigation(page);
  const trash = page.getByRole("region", { name: "Trash", exact: true });
  if (!(await trash.isVisible()))
    await navigation.getByRole("button", { name: "Trash", exact: true }).click();
  await expect(trash).toBeVisible();
  return trash;
}

export async function attachToNote(
  page: Page,
  files: Parameters<FileChooser["setFiles"]>[0],
): Promise<void> {
  const note = page.getByLabel("Note", { exact: true });
  await pressShortcut(note, "End");
  await note.press("Enter");
  await note.press("Enter");
  await note.pressSequentially("/file");
  const choosing = page.waitForEvent("filechooser");
  await note.press("Enter");
  await (await choosing).setFiles(files);
  await expect(note).not.toHaveMarkdown(/(?:^|\n)\/file$/);
  await expect(page.locator("#save-status")).toHaveText("Saved");
}

export async function newEmptyNote(page: Page): Promise<void> {
  await newNote(page);
  await closeNavigation(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await clearMarkdown(editor);
  await expect(page.locator("#body")).toHaveValue("");
}
