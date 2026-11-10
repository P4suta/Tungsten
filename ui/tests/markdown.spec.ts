import { accessibilityViolations } from "./accessibility.js";
import { expect, pressShortcut, test } from "./editor.js";
import { missingIcons } from "./icons-check.js";
import { newEmptyNote, openNavigation } from "./interface.js";

test("keeps the note menu compact with icons and keyboard access", async ({ page }, info) => {
  await page.goto("/");
  const more = await openNavigation(page);
  await expect(more.getByRole("combobox")).toHaveCount(0);
  expect(await page.evaluate(missingIcons, null)).toEqual([]);
  expect(await accessibilityViolations(page)).toEqual([]);
  await page.screenshot({ path: `../target/screenshots/menu-compact-${info.project.name}.png` });
});

test("renders list markers, quotes, and literal fenced code without replacing editable lines", async ({
  page,
}, info) => {
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  const text =
    "# Blocks\n\n- Minus\n+ Plus\n* Star\n\n> Quoted text\n\n```ts\nconst value = '*literal*';\n- code, not a list\n```\n\nAfter";
  await pressShortcut(editor, "a");
  await page.keyboard.insertText(text);
  await expect(page.locator(".live-list-marker")).toHaveText(["•", "•", "•"]);
  await expect(page.locator(".live-quote")).toContainText("Quoted text");
  await expect(page.locator(".live-quote")).not.toContainText(">");
  await expect(page.locator(".live-code-block").filter({ hasText: "const value" })).toHaveCSS(
    "font-family",
    /monospace/,
  );
  await expect(page.locator(".live-code-block em")).toHaveCount(0);
  await expect(page.locator(".live-code-fence")).toHaveText(["ts", ""]);
  await expect(page.locator("#body")).toHaveValue(text);
  await page.screenshot({
    path: `../target/screenshots/markdown-blocks-${info.project.name}.png`,
    fullPage: true,
  });
  const list = page.locator(".live-list-line").first();
  const top = await list.evaluate(
    (line) =>
      line.getBoundingClientRect().top + (document.querySelector("#paper-scroll")?.scrollTop ?? 0),
  );
  await list.click();
  await editor.press("Home");
  await expect(page.locator(".live-list-line").first()).toContainText("- Minus");
  await pressShortcut(editor, "Home");
  await expect(page.locator(".live-list-marker").first()).toHaveText("•");
  expect(
    await list.evaluate(
      (line) =>
        line.getBoundingClientRect().top +
        (document.querySelector("#paper-scroll")?.scrollTop ?? 0),
    ),
  ).toBe(top);
});

test("exits empty list and quote containers and preserves code as literal input", async ({
  page,
}) => {
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  for (const marker of ["-", "+", "*", "- ", "+ ", "* ", "1. ", "- [ ] ", ">", "> "]) {
    await pressShortcut(editor, "a");
    await page.keyboard.insertText(`# Empty\n\n${marker}`);
    await editor.press("Enter");
    await expect(page.locator("#body")).toHaveValue("# Empty\n\n");
  }
  await pressShortcut(editor, "a");
  await page.keyboard.insertText("# Code\n\n```md\n- literal");
  await editor.press("Enter");
  await expect(page.locator("#body")).toHaveValue("# Code\n\n```md\n- literal\n");
  await pressShortcut(editor, "a");
  await page.keyboard.insertText("# Quote\n\n> - [x] Task");
  await editor.press("Enter");
  await expect(page.locator("#body")).toHaveValue("# Quote\n\n> - [x] Task\n> - [ ] ");
  await editor.press("Enter");
  await expect(page.locator("#body")).toHaveValue("# Quote\n\n> - [x] Task\n> ");
  await editor.press("Enter");
  await expect(page.locator("#body")).toHaveValue("# Quote\n\n> - [x] Task\n");
});

test("suggests note titles while entering a Markdown link and keeps stable link targets", async ({
  page,
  request,
}, info) => {
  const target = `suggested-${info.project.name}.md`;
  const title = `Reference ${info.project.name}`;
  await request.post("/api/notes", { data: { name: target, body: `# ${title}` } });
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.pressSequentially("# Links");
  await editor.press("Enter");
  await editor.press("Enter");
  await expect(page.locator("#body")).toHaveValue("# Links\n\n");
  await editor.pressSequentially(`[${title}`);
  await expect(page.locator("#body")).toHaveValue(`# Links\n\n[${title}]()`);
  const suggestions = page.getByRole("listbox", { name: "Link to note", exact: true });
  await expect(suggestions).toBeVisible();
  await expect(suggestions.getByRole("option")).toHaveText([title]);
  await editor.press("ArrowDown");
  await editor.press("Enter");
  await expect(page.locator("#body")).toHaveValue(`# Links\n\n[${title}](${target})`);
  await expect(suggestions).toBeHidden();
  await editor.pressSequentially(" continued");
  await expect(page.locator("#body")).toHaveValue(`# Links\n\n[${title}](${target}) continued`);
  await editor.press("Enter");
  await editor.pressSequentially(`[${title}`);
  await expect(suggestions).toBeVisible();
  await editor.press("Escape");
  await expect(suggestions).toBeHidden();
  await expect(page.locator("#body")).toHaveValue(
    `# Links\n\n[${title}](${target}) continued\n[${title}]()`,
  );
});

test("keeps formatting automatic without a format action in the note menu", async ({ page }) => {
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await pressShortcut(editor, "a");
  await page.keyboard.insertText("# Auto\n\n* item");
  const more = await openNavigation(page);
  await expect(more.getByRole("button", { name: "Format Markdown", exact: true })).toHaveCount(0);
  await expect(page.locator("#body")).toHaveValue("# Auto\n\n- item\n");
});

test("applies consecutive Enter presses exactly once before further typing", async ({ page }) => {
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.pressSequentially("# Lines");
  for (let count = 0; count < 6; count++) await editor.press("Enter");
  await expect(page.locator("#body")).toHaveValue("# Lines\n\n\n\n\n\n");
  await editor.pressSequentially("Body");
  await expect(page.locator("#body")).toHaveValue("# Lines\n\n\n\n\n\nBody");
});

test("suggests links in the editor without requiring a typing help preference", async ({
  page,
  request,
}, info) => {
  const title = `Source reference ${info.project.name}`;
  const target = `source ${info.project.name}.md`;
  await request.post("/api/notes", { data: { name: target, body: `# ${title}` } });
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByLabel("Note", { exact: true });
  await editor.fill("# Source\n\n");
  await pressShortcut(editor, "End");
  await editor.pressSequentially(`[${title}`);
  const suggestions = page.getByRole("listbox", { name: "Link to note", exact: true });
  await expect(suggestions.getByRole("option")).toHaveText([title]);
  expect(await accessibilityViolations(page)).toEqual([]);
  await editor.press("Enter");
  await expect(editor).toHaveMarkdown(`# Source\n\n[${title}](${encodeURIComponent(target)})`);
  const more = await openNavigation(page);
  await expect(more.getByRole("link", { name: "Settings", exact: true })).toHaveCount(0);
  await more.getByRole("button", { name: "Close navigation", exact: true }).click();
  await editor.fill("# Source\n\n");
  await pressShortcut(editor, "End");
  await editor.pressSequentially(`[${title}`);
  await expect(suggestions).toBeVisible();
  await expect(editor).toHaveMarkdown(`# Source\n\n[${title}]()`);
});
