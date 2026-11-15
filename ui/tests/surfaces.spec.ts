import { accessibilityViolations } from "./accessibility.js";
import { expect, pressShortcut, test } from "./editor.js";
import { clippedIcons } from "./icon-bounds.js";
import { missingIcons } from "./icons-check.js";
import { openContext, openNavigation, openTrash } from "./interface.js";

test("reloads Trash after returning while an earlier request is pending", async ({
  page,
  request,
}) => {
  const created = await request.post("/api/notes", {
    data: { name: "Pending-trash.md", body: "# Pending deleted note\n" },
  });
  const saved: { revision: string } = await created.json();
  await request.delete("/api/notes/Pending-trash.md", {
    headers: { "if-match": saved.revision, "content-type": "application/json" },
  });
  let release: () => void = () => {};
  const waiting = new Promise<void>((resolve) => {
    release = resolve;
  });
  let intercepted = false;
  await page.route("**/api/trash?**", async (route) => {
    if (!intercepted) {
      intercepted = true;
      await waiting;
    }
    await route.continue();
  });
  await page.goto("/");
  const trash = await openTrash(page);
  await expect.poll(() => intercepted).toBe(true);
  await page.keyboard.press("Escape");
  await openTrash(page);
  release();
  await expect(
    trash.getByRole("option", { name: "Pending deleted note", exact: true }),
  ).toBeVisible();
});

test("selects, previews, pages, and restores deleted notes inside navigation", async ({
  page,
  request,
}, info) => {
  expect((await request.delete("/api/trash", { data: {} })).ok()).toBe(true);
  for (let index = 0; index < 57; index++) {
    const name = `trash-${info.project.name}-${index}.md`;
    const created = await request.post("/api/notes", {
      data: { name, body: `# Discarded ${String(index).padStart(3, "0")}\n\nBody\n` },
    });
    const saved: { revision: string } = await created.json();
    expect(
      (
        await request.delete(`/api/notes/${name}`, {
          headers: { "content-type": "application/json", "if-match": saved.revision },
        })
      ).ok(),
    ).toBe(true);
  }
  await page.goto("/");
  const trash = await openTrash(page);
  expect(new URL(page.url()).pathname).toBe("/");
  await expect(trash.getByRole("button", { name: "Delete permanently", exact: true })).toHaveCount(
    0,
  );
  const rows = trash.getByRole("option");
  await expect(rows).toHaveCount(50);
  await expect(trash.locator("#restore-trashed svg")).toHaveAttribute("data-icon", "undo");
  await expect(trash.locator("#trash-next svg")).toHaveAttribute("data-icon", "next");
  const selected = trash.getByRole("option", { name: "Discarded 056", exact: true });
  await selected.click();
  await expect(selected).toHaveAttribute("aria-selected", "true");
  await selected.press("Enter");
  const preview = page.getByRole("region", { name: "Deleted note", exact: true });
  await expect(preview.getByLabel("Deleted content", { exact: true })).toHaveValue(
    "# Discarded 056\n\nBody\n",
  );
  await preview.getByRole("button", { name: "Close deleted note preview", exact: true }).click();
  await expect(trash.getByRole("button", { name: "Preview", exact: true })).toBeFocused();
  await selected.focus();
  await selected.press("End");
  await expect(rows.last()).toBeFocused();
  await trash.getByRole("button", { name: "Next page", exact: true }).click();
  await expect(trash.getByRole("option", { name: "Discarded 000", exact: true })).toBeVisible();
  await trash.getByRole("button", { name: "Previous page", exact: true }).click();
  await selected.click();
  expect(await accessibilityViolations(page)).toEqual([]);
  expect(await page.evaluate(missingIcons, null)).toEqual([]);
  await page.screenshot({
    path: `../target/screenshots/trash-${info.project.name}.png`,
    fullPage: true,
  });
  await trash.getByRole("button", { name: "Restore", exact: true }).click();
  await expect(page.getByRole("textbox", { name: "Note", exact: true })).toHaveMarkdown(
    "# Discarded 056\n\nBody\n",
  );
  await openTrash(page);
  await expect(trash.getByRole("option", { name: "Discarded 056", exact: true })).toHaveCount(0);
});

test("shows the effect of restoring a writing session without interpreting its source", async ({
  page,
  request,
}, info) => {
  const name = `difference-${info.project.name}.md`;
  await request.post("/api/notes", {
    data: { name, body: "# Session\n\nOriginal <script>literal</script>\n" },
  });
  await page.goto(`/?note=${name}`);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.fill("# Session\n\nRevised text\n");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const more = await openContext(page);
  await more.getByRole("button", { name: "Version history", exact: true }).click();
  const history = page.getByRole("region", { name: "Version history", exact: true });
  await history.getByRole("button", { name: "View version", exact: true }).first().click();
  await expect(history.locator("#version-diff")).toContainText(
    "+ Original <script>literal</script>",
  );
  await expect(history.locator("#version-diff")).toContainText("- Revised text");
  await expect(history.locator("#version-diff script")).toHaveCount(0);
  await expect(history.getByRole("button", { name: "Restore version", exact: true })).toBeEnabled();
  expect(await accessibilityViolations(page)).toEqual([]);
});

test("keeps icons inside their view boxes and rejects a clipped fixture", async ({
  page,
  request,
}, info) => {
  const name = `icons-${info.project.name}.md`;
  await request.post("/api/notes", {
    data: { name, body: "# Icons\n\n[A link](https://example.com)\n\nEnd\n" },
  });
  await page.goto(`/?note=${name}`);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await pressShortcut(editor, "End");
  await expect(page.locator("svg[data-icon=link]").first()).toBeVisible();
  expect(await page.evaluate(clippedIcons, null)).toEqual([]);
  expect(await page.evaluate(clippedIcons, "clipped" as const)).toContain("bounds-fixture");
  expect(await page.evaluate(clippedIcons, "padded" as const)).toEqual([]);
  await openNavigation(page);
  expect(await page.evaluate(clippedIcons, null)).toEqual([]);
});
