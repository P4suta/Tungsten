import type { FileChooser, Locator, Page } from "@playwright/test";
import { expect, markdownValue } from "./editor.js";

export async function openMore(page: Page): Promise<Locator> {
  const dialog = page.getByRole("dialog", { name: "More", exact: true });
  if (!(await dialog.isVisible()))
    await page.getByRole("button", { name: "More", exact: true }).click();
  return dialog;
}

export async function closeMore(page: Page): Promise<void> {
  const dialog = page.getByRole("dialog", { name: "More", exact: true });
  if (await dialog.isVisible())
    await dialog.getByRole("button", { name: "Close", exact: true }).click();
}

export async function noteAction(page: Page, name: string): Promise<void> {
  const dialog = await openMore(page);
  const section = dialog.locator("#note-tools");
  await section.getByRole("button", { name, exact: true }).click();
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
  await (await openMore(page)).getByRole("link", { name: "Trash", exact: true }).click();
  const trash = page.getByRole("main", { name: "Trash", exact: true });
  await expect(trash).toBeVisible();
  return trash;
}

export async function attachToNote(
  page: Page,
  files: Parameters<FileChooser["setFiles"]>[0],
): Promise<void> {
  const note = page.getByLabel("Note", { exact: true });
  await note.press("ControlOrMeta+End");
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
  await page.getByRole("button", { name: "New note", exact: true }).click();
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.press("ControlOrMeta+a");
  await expect
    .poll(() =>
      page
        .locator("#body")
        .evaluate(
          (node: HTMLTextAreaElement) =>
            node.selectionStart === 0 && node.selectionEnd === node.value.length,
        ),
    )
    .toBe(true);
  await editor.press("Backspace");
  await expect(page.locator("#body")).toHaveValue("");
}
