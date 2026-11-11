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

export async function selectMarkdown(editor: Locator, from: number, to = from): Promise<void> {
  const source = await markdownValue(editor);
  if (from < 0 || to < from || to > source.length) throw new Error("Invalid Markdown selection");
  await editor.focus();
  await pressShortcut(editor, "Home");
  const backing = editor.page().locator("#body");
  const position = (edge: "start" | "end"): Promise<number> =>
    backing.evaluate(
      (node: HTMLTextAreaElement, edge) =>
        edge === "start" ? node.selectionStart : node.selectionEnd,
      edge,
    );
  await baseExpect.poll(() => position("start")).toBe(0);
  let cursor = 0;
  for (const character of Array.from(source.slice(0, from))) {
    await editor.press("ArrowRight");
    cursor += character.length;
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
