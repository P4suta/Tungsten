import { expect as baseExpect, type Locator, type Page, test } from "@playwright/test";

export { test };

export async function pressShortcut(target: Locator | Page, key: string): Promise<void> {
  const page = "keyboard" in target ? target : target.page();
  const modifier = await page.evaluate(() =>
    /Mac|iPhone|iPad|iPod/.test(navigator.platform) || /iPhone|iPad|iPod/.test(navigator.userAgent)
      ? "Meta"
      : "Control",
  );
  const shortcut = `${modifier}+${key}`;
  if ("keyboard" in target) await target.keyboard.press(shortcut);
  else await target.press(shortcut);
}

export async function markdownValue(editor: Locator): Promise<string> {
  return editor.page().locator("#body").inputValue();
}

export async function selectAllMarkdown(editor: Locator): Promise<void> {
  await pressShortcut(editor, "a");
  await baseExpect
    .poll(() =>
      editor
        .page()
        .locator("#body")
        .evaluate(
          (node: HTMLTextAreaElement) =>
            node.selectionStart === 0 && node.selectionEnd === node.value.length,
        ),
    )
    .toBe(true);
}

export async function clearMarkdown(editor: Locator): Promise<void> {
  await selectAllMarkdown(editor);
  await editor.press("Backspace");
}

export async function replaceMarkdown(editor: Locator, text: string): Promise<void> {
  await baseExpect(editor).toBeVisible();
  await baseExpect(editor).toBeEditable();
  await selectAllMarkdown(editor);
  if (text) await editor.page().keyboard.insertText(text);
  else await editor.press("Backspace");
  await baseExpect(editor.page().locator("#body")).toHaveValue(text);
}

export async function selectMarkdown(editor: Locator, from: number, to = from): Promise<void> {
  const source = await markdownValue(editor);
  if (from < 0 || to < from || to > source.length) throw new Error("Invalid Markdown selection");
  await editor.focus();
  const fromEnd = source.length - from < from;
  await pressShortcut(editor, fromEnd ? "End" : "Home");
  const backing = editor.page().locator("#body");
  const position = (edge: "start" | "end"): Promise<number> =>
    backing.evaluate(
      (node: HTMLTextAreaElement, edge) =>
        edge === "start" ? node.selectionStart : node.selectionEnd,
      edge,
    );
  let cursor = fromEnd ? source.length : 0;
  await baseExpect.poll(() => position("start")).toBe(cursor);
  const characters = Array.from(fromEnd ? source.slice(from) : source.slice(0, from));
  if (fromEnd) characters.reverse();
  for (const character of characters) {
    await editor.press(fromEnd ? "ArrowLeft" : "ArrowRight");
    cursor += fromEnd ? -character.length : character.length;
    await baseExpect.poll(() => position("start")).toBe(cursor);
  }
  for (const character of Array.from(source.slice(from, to))) {
    await editor.press("Shift+ArrowRight");
    cursor += character.length;
    await baseExpect.poll(() => position("end")).toBe(cursor);
  }
}

export const expect = baseExpect.extend({
  async toHaveMarkdown(editor: Locator, expected: string | RegExp, options?: { timeout?: number }) {
    await baseExpect(editor).toBeVisible(options);
    const assertion = this.isNot
      ? baseExpect(editor.page().locator("#body")).not
      : baseExpect(editor.page().locator("#body"));
    let pass = true;
    let message = "The Markdown source matched the unexpected value.";
    try {
      await assertion.toHaveValue(expected, options);
    } catch (error) {
      pass = false;
      message = error instanceof Error ? error.message : "The Markdown source did not match.";
    }
    return { pass: this.isNot ? !pass : pass, message: () => message };
  },
});
