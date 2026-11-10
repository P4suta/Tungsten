import { expect, test } from "@playwright/test";
import { accessibilityViolations } from "./accessibility.js";
import { exploreNotes, newEmptyNote, openNavigation } from "./interface.js";

test("keeps appearance, counts, and retired tools outside the note", async ({ page }, info) => {
  await page.goto("/");
  const more = await openNavigation(page);
  await expect(more.getByRole("link", { name: "Settings", exact: true })).toHaveCount(0);
  await expect(more.locator("select, #note-details, #import-files")).toHaveCount(0);
  await expect(
    more.getByRole("searchbox", { name: "Search all notes", exact: true }),
  ).toBeVisible();
  await expect(page.locator("#note-stats, #relations-dialog, #settings-page")).toHaveCount(0);
  for (const name of [
    "Find and replace",
    "Duplicate note",
    "Download Markdown",
    "Print note",
    "View relations",
    "Format Markdown",
  ])
    await expect(more.getByRole("button", { name, exact: true })).toHaveCount(0);
  await expect(more.getByRole("link", { name: "Download all files", exact: true })).toHaveCount(0);
  expect((await page.request.get("/settings")).status()).toBe(404);
  await page.screenshot({ path: `../target/screenshots/note-menu-${info.project.name}.png` });
  expect(await accessibilityViolations(page)).toEqual([]);
});

test("aligns headings with the body without a gutter or text counts", async ({ page, request }) => {
  const source =
    "# Folding\n\nIntro\n\n## First\n\nSecret paragraph\n\n### Nested\n\nNested body\n\n## Second\n\nVisible paragraph\n";
  await request.post("/api/notes", { data: { name: "Fold.md", body: source } });
  await page.goto("/?note=Fold.md");
  await expect(page.locator("#note-stats")).toHaveCount(0);
  await expect(page.locator(".heading-fold, .writing-icon")).toHaveCount(0);
  await expect(page.locator("#live-editor")).toContainText("Secret paragraph");
  await expect(page.locator("#live-editor")).toContainText("Nested body");
  const positions = await page
    .locator(".cm-line")
    .evaluateAll((lines) => lines.map((line) => line.getBoundingClientRect().left));
  expect(Math.max(...positions) - Math.min(...positions)).toBeLessThan(1);
  expect((await (await request.get("/api/notes/Fold.md")).json()).body).toBe(source);
  expect(await accessibilityViolations(page)).toEqual([]);
});

test("explores incoming and outgoing links in one graph", async ({ page, request }) => {
  for (const [name, body] of [
    ["Center.md", "# Center\n\n[Outgoing](Out.md)"],
    ["In.md", "# Incoming\n\n[Center](Center.md)"],
    ["Out.md", "# Outgoing\n\nEnd"],
  ])
    await request.post("/api/notes", { data: { name, body } });
  await page.goto("/?note=Center.md");
  await exploreNotes(page);
  const graph = page.getByRole("region", { name: "Note graph", exact: true });
  await expect(graph.getByRole("button", { name: "Explore Incoming", exact: true })).toBeVisible();
  await graph.getByRole("button", { name: "Explore Outgoing", exact: true }).click();
  await page.getByRole("link", { name: "Open note", exact: true }).click();
  await expect(page).toHaveURL(/note=Out.md/);
  await expect(page.getByRole("textbox", { name: "Note", exact: true })).toBeFocused();
});

test("checks a changed file link before saving and repairs its destination without losing text", async ({
  page,
  request,
}) => {
  const file: { id: string } = await (
    await request.post("/api/attachments", {
      data: { name: "source.txt", data: Buffer.from("original bytes").toString("base64") },
    })
  ).json();
  const source = `# Linked file\n\n[Source](.attachments/${file.id})\n`;
  await request.post("/api/notes", { data: { name: "File-check.md", body: source } });
  await page.goto("/?note=File-check.md");
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await expect(editor).toBeEditable();
  let release: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const invalid = "00000000000000000000000000000000.txt";
  await page.route(`**/api/attachments/${invalid}`, async (route) => {
    await held;
    await route.continue();
  });
  let updates = 0;
  page.on("request", (request) => {
    if (request.method() === "PUT") ++updates;
  });
  await editor.press("ControlOrMeta+a");
  await editor.press("Backspace");
  await page.keyboard.insertText(`${source.replace(file.id, invalid)}\nExtra writing\n`);
  await expect(page.locator("#body")).toHaveValue(
    `${source.replace(file.id, invalid)}\nExtra writing\n`,
  );
  const warning = page.locator("#file-link-warning");
  await expect(warning).toBeVisible();
  await expect(page.locator("#save-status")).toHaveText("Saving");
  expect(updates).toBe(0);
  release();
  await expect(warning).toContainText("cannot be opened");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await warning.getByRole("button", { name: "Repair file link", exact: true }).click();
  await expect(page.locator("#body")).toHaveValue(`${source}\nExtra writing\n`);
  await expect(warning).toBeHidden();
  expect(await (await request.get(`/.attachments/${file.id}`)).text()).toBe("original bytes");
  await editor.press("ControlOrMeta+a");
  await page.keyboard.insertText("# Linked file\n\nExtra writing\n");
  await expect(warning).toBeHidden();
});

test("copies Markdown source through rendered content and pastes it into a new note", async ({
  page,
  request,
}) => {
  const source = "# Copy\n\n## Section\n\n**Source** and a [link](https://example.com).\n";
  await request.post("/api/notes", { data: { name: "Copy.md", body: source } });
  await page.goto("/?note=Copy.md");
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await expect(editor).toBeEditable();
  await expect(page.locator("#body")).toHaveValue(source);
  await editor.press("ControlOrMeta+a");
  const copied = await editor.evaluate((input) => {
    const transfer = new DataTransfer();
    const event = new ClipboardEvent("copy", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: transfer });
    input.dispatchEvent(event);
    return transfer.getData("text/plain");
  });
  expect(copied).toBe(source);
  await newEmptyNote(page);
  await editor.evaluate((input, copied) => {
    const transfer = new DataTransfer();
    transfer.setData("text/plain", copied);
    const event = new ClipboardEvent("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: transfer });
    input.dispatchEvent(event);
  }, copied);
  await expect(page.locator("#body")).toHaveValue(source);
});

test("preserves single line breaks in rendered paragraphs with links", async ({
  page,
  request,
}) => {
  const source = "# Breaks\n\nOne [link](https://example.com)\nTwo\n\nAfter\n";
  await request.post("/api/notes", { data: { name: "Breaks.md", body: source } });
  await page.goto("/?note=Breaks.md");
  await page.getByRole("textbox", { name: "Note", exact: true }).press("ControlOrMeta+End");
  await expect(page.locator(".live-block br")).toHaveCount(1);
  await expect(page.locator(".live-block")).toContainText("One link");
  await expect(page.locator(".live-block")).toContainText("Two");
});

test("retracts an accidental attachment with cancellation and permanent original deletion", async ({
  page,
  request,
}) => {
  const file: { id: string } = await (
    await request.post("/api/attachments", {
      data: { name: "accidental.txt", data: Buffer.from("accidental bytes").toString("base64") },
    })
  ).json();
  const source = `# Retract\n\n[file](.attachments/${file.id})\n\nKeep writing\n`;
  await request.post("/api/notes", { data: { name: "Retract.md", body: source } });
  await page.goto("/?note=Retract.md");
  await page.getByRole("textbox", { name: "Note", exact: true }).press("ControlOrMeta+End");
  await page.getByText("File options", { exact: true }).click();
  page.once("dialog", (dialog) => {
    void dialog.dismiss();
  });
  await page.getByRole("button", { name: "Delete file permanently", exact: true }).click();
  expect((await request.get(`/.attachments/${file.id}`)).ok()).toBe(true);
  page.once("dialog", (dialog) => {
    void dialog.accept();
  });
  await page.getByRole("button", { name: "Delete file permanently", exact: true }).click();
  await expect(page.locator("#body")).not.toHaveValue(new RegExp(file.id));
  await expect(page.locator("#body")).toHaveValue(/Keep writing/);
  await expect(page.locator("#save-status")).toHaveText("Saved");
  expect((await request.get(`/.attachments/${file.id}`)).status()).toBe(404);
  expect((await request.get(`/api/attachments/${file.id}`)).status()).toBe(404);
});
