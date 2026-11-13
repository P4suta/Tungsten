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
  const sidebar = page.getByRole("complementary", { name: "Navigation", exact: true });
  if (!(await sidebar.isVisible()))
    await page.getByRole("button", { name: "Navigation", exact: true }).click();
  await sidebar.getByRole("button", { name: "Close navigation", exact: true }).focus();
  return sidebar;
}

export async function closeNavigation(page: Page): Promise<void> {
  const sidebar = page.getByRole("complementary", { name: "Navigation", exact: true });
  if (await sidebar.isVisible())
    await sidebar.getByRole("button", { name: "Close navigation", exact: true }).click();
}

export async function newNote(page: Page): Promise<void> {
  await (await openNavigation(page)).getByRole("button", { name: "New note", exact: true }).click();
  await expect(page.locator("#paper-scroll")).toBeVisible();
}

export async function exploreNotes(page: Page): Promise<void> {
  await (await openNavigation(page))
    .getByRole("button", { name: "Explore notes", exact: true })
    .click();
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
  const sidebar = page.getByRole("complementary", { name: "Current note", exact: true });
  if (!(await sidebar.isVisible()))
    await page.getByRole("button", { name: "Note context", exact: true }).click();
  return sidebar;
}

export async function showPaper(page: Page): Promise<void> {
  if (!(await page.locator("#paper-scroll").isVisible()))
    await page.getByRole("button", { name: "Write note", exact: true }).click();
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
  await (await openNavigation(page)).getByRole("button", { name: "Trash", exact: true }).click();
  const trash = page.getByRole("region", { name: "Trash", exact: true });
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
