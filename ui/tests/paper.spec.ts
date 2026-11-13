import { clearMarkdown, expect, markdownValue, pressShortcut, test } from "./editor.js";
import { newEmptyNote, newNote, openNavigation } from "./interface.js";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByLabel("Note", { exact: true })).toBeEditable();
  await newEmptyNote(page);
});

test("starts with a blank paper without scrolling or persistent tools", async ({ page }, info) => {
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown("");
  await expect(
    page.locator("main button:visible, main input:visible, main summary:visible"),
  ).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Today", exact: true })).toHaveCount(0);
  await expect(page.getByRole("navigation")).toHaveCount(0);
  await expect(page.locator("[data-format]")).toHaveCount(0);
  for (const viewport of [
    { width: 1280, height: 800 },
    { width: 320, height: 700 },
    { width: 800, height: 320 },
  ]) {
    await page.setViewportSize(viewport);
    await expect
      .poll(() =>
        page.evaluate(() => ({
          width: document.documentElement.scrollWidth <= innerWidth,
          height:
            document.documentElement.scrollHeight <= innerHeight + 1 &&
            [...document.querySelectorAll<HTMLElement>("#paper-scroll")].every(
              (paper) => paper.scrollHeight <= paper.clientHeight + 1,
            ),
        })),
      )
      .toEqual({ width: true, height: true });
  }
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "225%";
  });
  await expect
    .poll(() =>
      page.evaluate(() => ({
        within:
          document.documentElement.scrollHeight <= innerHeight + 1 &&
          [...document.querySelectorAll<HTMLElement>("#paper-scroll")].every(
            (paper) => paper.scrollHeight <= paper.clientHeight + 1,
          ),
        viewport: innerHeight,
        height: document.documentElement.scrollHeight,
        elements: ["header", "#editor-pane", ".cm-content", ".cm-line", "#typing-space"].map(
          (selector) => {
            const element = document.querySelector<HTMLElement>(selector);
            const rect = element?.getBoundingClientRect();
            return {
              selector,
              top: rect?.top,
              height: rect?.height,
              style: element ? getComputedStyle(element).minHeight : "",
            };
          },
        ),
      })),
    )
    .toMatchObject({ within: true });
  await page.evaluate(() => document.documentElement.style.removeProperty("font-size"));
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.screenshot({
    path: `../target/screenshots/blank-paper-${info.project.name}.png`,
    fullPage: true,
  });
});

test("completes delimiters and wraps a selection without using a toolbar", async ({ page }) => {
  const note = page.getByLabel("Note", { exact: true });
  await note.press("*");
  await expect(note).toHaveMarkdown("**");
  await note.press("Backspace");
  await expect(note).toHaveMarkdown("");
  await note.press("[");
  await expect(note).toHaveMarkdown("[]()");
  await note.fill("selected text");
  await pressShortcut(note, "a");
  await note.press("*");
  await expect(note).toHaveMarkdown("**selected text**");
  await pressShortcut(note, "z");
  await expect(note).toHaveMarkdown("selected text");
  await pressShortcut(note, "a");
  await pressShortcut(note, "b");
  await expect(note).toHaveMarkdown("**selected text**");
  await pressShortcut(note, "b");
  await expect(note).toHaveMarkdown("selected text");
  await clearMarkdown(note);
  await note.pressSequentially("**bold**");
  await expect(note).toHaveMarkdown("**bold**");
  await clearMarkdown(note);
  await note.press("[");
  await note.pressSequentially("Example");
  await note.press("]");
  await note.pressSequentially("https://example.com");
  await note.press(")");
  await expect(note).toHaveMarkdown("[Example](https://example.com)");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await page.reload();
  await expect(note).toHaveMarkdown("[Example](https://example.com)");
});

test("continues Markdown lists and lets Tab leave the editor after Escape", async ({ page }) => {
  const note = page.getByLabel("Note", { exact: true });
  await note.fill("- [x] done");
  await note.press("End");
  await note.press("Enter");
  await expect(note).toHaveMarkdown("- [x] done\n- [ ] ");
  await note.press("Enter");
  await expect(note).toHaveMarkdown("- [x] done\n");
  await note.fill("first\nsecond");
  await pressShortcut(note, "a");
  await note.press("Tab");
  await expect(note).toHaveMarkdown("    first\n    second");
  await note.press("Shift+Tab");
  await expect(note).toHaveMarkdown("first\nsecond");
  await note.press("Escape");
  await note.press("Shift+Tab");
  await expect(page.getByRole("button", { name: "Note context", exact: true })).toBeFocused();
});

test("leaves composition unchanged with automatic typing assistance", async ({ page }) => {
  const note = page.getByLabel("Note", { exact: true });
  const accepted = await page.locator("#body").evaluate((node: HTMLTextAreaElement) => {
    node.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    const event = new InputEvent("beforeinput", {
      bubbles: true,
      cancelable: true,
      inputType: "insertText",
      data: "*",
      isComposing: true,
    });
    const accepted = node.dispatchEvent(event);
    node.value = "変換*";
    node.dispatchEvent(new InputEvent("input", { bubbles: true, isComposing: true }));
    node.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true }));
    return accepted;
  });
  expect(accepted).toBe(true);
  await expect(note).toHaveMarkdown("変換*");
  await pressShortcut(note, "a");
  await note.press("Backspace");
  await expect(note).toHaveMarkdown("");
  await note.press("*");
  await expect(note).toHaveMarkdown("**");
});

test("always opens an editable live note and ignores retired view preferences", async ({
  page,
}) => {
  await page.evaluate(() => localStorage.setItem("tungsten.view", "preview"));
  await page.reload();
  await newNote(page);
  const note = page.getByRole("textbox", { name: "Note", exact: true });
  await expect(note).toBeEditable();
  await note.fill("# Next note");
  await pressShortcut(note, "Enter");
  await expect(note).toBeFocused();
  await expect(note).toBeEditable();
  const before = await markdownValue(note);
  await note.pressSequentially("!");
  await expect(note).toHaveMarkdown(`${before}!`);
  const more = await openNavigation(page);
  await expect(more.getByLabel("View", { exact: true })).toHaveCount(0);
  await expect(more.getByRole("link", { name: "Download all files", exact: true })).toHaveCount(0);
  await expect(page.locator("#preview-pane, .writing-icon, .heading-fold")).toHaveCount(0);
});
