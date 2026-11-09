import { expect, selectMarkdown, test } from "./editor.js";
import { newEmptyNote, openMore } from "./interface.js";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await newEmptyNote(page);
});

test("normalizes heading ranks automatically as one undoable edit", async ({ page }) => {
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  const original =
    "### Title\n\n###### First\n\n# Second\n\n##### Third\n\n```md\n# literal\n###### literal\n```";
  await editor.fill(original);
  const more = await openMore(page);
  await expect(editor).toHaveMarkdown(
    "# Title\n\n## First\n\n## Second\n\n### Third\n\n```md\n# literal\n###### literal\n```\n",
  );
  await expect(page.getByRole("heading", { level: 1 })).toHaveCount(1);
  await expect(editor.getByRole("heading", { level: 2 })).toHaveCount(2);
  await more.getByRole("button", { name: "Close", exact: true }).click();
  await editor.press("ControlOrMeta+z");
  await expect(editor).toHaveMarkdown(original);
});

test("unwraps matched inline syntax while preserving contents and native undo", async ({
  page,
}) => {
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  for (const marker of ["*", "**", "_", "__", "~~", "`", "``"]) {
    const original = `# Pairs\n\n${marker}日本語 😀${marker}\n`;
    await editor.fill(original);
    await expect(editor).toHaveMarkdown(original);
    await selectMarkdown(editor, 9, 9 + marker.length);
    await editor.press("Backspace");
    await expect(editor).toHaveMarkdown("# Pairs\n\n日本語 😀\n");
    await editor.press("ControlOrMeta+z");
    await expect(editor).toHaveMarkdown(original);
  }
});

test("removes the matching fence when either fence is deleted", async ({ page }) => {
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  for (const marker of ["```", "~~~", "````"]) {
    const original = `# Fence\n\n${marker}python extra\nimport package\n${marker}\n\nTail`;
    await editor.fill(original);
    await expect(editor).toHaveMarkdown(original);
    await selectMarkdown(editor, 9 + marker.length);
    await editor.press("Backspace");
    await expect(editor).toHaveMarkdown("# Fence\n\nimport package\n\nTail");
    await editor.press("ControlOrMeta+z");
    await expect(editor).toHaveMarkdown(original);
    await selectMarkdown(editor, original.lastIndexOf(marker));
    await editor.press("Delete");
    await expect(editor).toHaveMarkdown("# Fence\n\nimport package\n\nTail");
  }
});

test("highlights bundled languages and aliases without interpreting literal HTML", async ({
  page,
}, info) => {
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.fill(
    "# Code\n\n```js extra\nconst answer = 42;\n```\n\n```python\nimport package\n```\n\n```html\n<script>throw Error('literal')</script>\n```\n\n```hogepiyo lang\nconst literal = '*raw*';\n```\n\nEnd",
  );
  await editor.press("ControlOrMeta+Home");
  await expect(page.locator(".live-code-block .hljs-keyword")).toHaveText([
    "const",
    "import",
    "throw",
  ]);
  await expect(page.locator(".live-code-block .hljs-name")).toHaveText(["script", "script"]);
  await expect(page.locator(".live-code-block").filter({ hasText: "const literal" })).toHaveText(
    "const literal = '*raw*';",
  );
  await expect(
    page.locator(".live-code-block").filter({ hasText: "const literal" }).locator("[class*=hljs-]"),
  ).toHaveCount(0);
  await expect(page.locator("#live-editor script")).toHaveCount(0);
  await expect(page.locator(".live-code-block .hljs-keyword").first()).toHaveCSS(
    "font-weight",
    "700",
  );
  await page.screenshot({
    path: `../target/screenshots/syntax-${info.project.name}.png`,
    fullPage: true,
  });
});
