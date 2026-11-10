import { accessibilityViolations } from "./accessibility.js";
import { expect, pressShortcut, selectMarkdown, test } from "./editor.js";
import {
  attachToNote,
  exploreNotes,
  nameNote,
  newNote,
  noteAction,
  openNavigation,
  openTrash,
} from "./interface.js";

test.beforeEach(async ({ page, request }) => {
  const notes: { name: string }[] = await (await request.get("/api/notes")).json();
  for (const note of notes) {
    const path = `/api/notes/${encodeURIComponent(note.name)}`;
    const saved = await request.get(path);
    await request.delete(path, {
      headers: { "content-type": "application/json", "if-match": saved.headers().etag ?? "" },
    });
  }
  const trash: { id: string; revision: string }[] = await (await request.get("/api/trash")).json();
  for (const note of trash) {
    await request.delete(`/api/trash/${note.id}`, {
      headers: { "content-type": "application/json", "if-match": note.revision },
    });
  }
  page.on("dialog", (dialog) => {
    void dialog.accept();
  });
  await page.goto("/");
  await expect(page.getByLabel("Note", { exact: true })).toBeEditable();
});

test("previews Markdown, follows note links, and toggles source tasks", async ({
  page,
  request,
}) => {
  await request.post("/api/notes", { data: { name: "Linked.md", body: "Linked content" } });
  await nameNote(page, "Markdown");
  await page
    .getByLabel("Note", { exact: true })
    .fill("# 日本語😀\n\n**Bold**\n\n- [ ] Todo\n\n[[Linked]]\n\n<script>alert('unsafe')</script>");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await expect(page.locator("#live-editor strong")).toHaveText("Bold");
  await expect(page.locator("#live-editor script")).toHaveCount(0);
  await page
    .locator("#live-editor")
    .getByRole("checkbox", { name: "Task: Todo", exact: true })
    .check();
  await expect(page.locator("#save-status")).toHaveText("Saved");
  expect((await (await request.get("/api/notes/Markdown.md")).json()).body).toContain("- [x] Todo");
  await page.locator("#live-editor").getByRole("link", { name: "Linked", exact: true }).click();
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown("Linked content");
  await exploreNotes(page);
  await expect(page.getByRole("button", { name: "Explore 日本語😀", exact: true })).toBeVisible();
});

test("searches a flat note collection and leaves browser Find available", async ({ page }) => {
  await nameNote(page, "Projects/Plan");
  await page.getByLabel("Note", { exact: true }).fill("alpha alpha #work");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  expect(
    await page.evaluate(() => {
      const event = new KeyboardEvent("keydown", {
        key: "f",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      });
      document.dispatchEvent(event);
      return event.defaultPrevented;
    }),
  ).toBe(false);
  await exploreNotes(page);
  await page.getByLabel("Search notes", { exact: true }).fill("alpha");
  await expect(page.locator("#notes")).toContainText("alpha alpha #work");
  await expect(page.locator("#notes")).not.toContainText("Projects/Plan");
});

test("restores note history and deleted notes through the browser", async ({
  page,
  request,
}, info) => {
  const name = `History-${info.project.name}`;
  await nameNote(page, name);
  await page.getByLabel("Note", { exact: true }).fill("original text\n");
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown("original text\n");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await page.getByLabel("Note", { exact: true }).fill("changed text\n");
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown("changed text\n");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await noteAction(page, "Version history");
  const history = page.getByRole("region", { name: "Version history", exact: true });
  await history.getByRole("button", { name: "View version" }).first().click();
  await expect(history.getByLabel("Saved content", { exact: true })).toHaveValue("original text\n");
  await history.getByRole("button", { name: "Restore version", exact: true }).click();
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown("original text\n");
  await noteAction(page, "Delete");
  await expect(page.locator("#save-status")).toHaveText("Ready");
  await expect(page.getByRole("button", { name: "Navigation", exact: true })).toBeInViewport();
  const library = await openNavigation(page);
  await library.getByRole("button", { name: "Trash", exact: true }).click();
  const trash = page.getByRole("region", { name: "Trash", exact: true });
  await trash.getByRole("option", { name: "original text", exact: true }).click();
  await trash.getByRole("button", { name: "Restore", exact: true }).click();
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown("original text\n");
  expect((await request.get(`/api/notes/${name}.md`)).ok()).toBeTruthy();
});

test("opens original attachments directly without a bulk download action", async ({
  page,
  request,
}) => {
  await request.post("/api/notes", {
    data: { name: "Templates/Daily.md", body: "## {{title}}\n\n{{date}}\n" },
  });
  const library = await openNavigation(page);
  await library.getByRole("button", { name: "Close navigation", exact: true }).click();
  await newNote(page);
  await page.getByLabel("Note", { exact: true }).fill("File: ");
  await attachToNote(page, {
    name: "hello.txt",
    mimeType: "text/plain",
    buffer: Buffer.from("hello file"),
  });
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown(/\.attachments\/.+\.txt/);
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await pressShortcut(page.getByLabel("Note", { exact: true }), "Home");
  const fileLink = page
    .locator("#live-editor")
    .getByRole("link", { name: "hello.txt", exact: true })
    .first();
  await expect(fileLink).toBeVisible();
  const href = await fileLink.locator("a").getAttribute("href");
  expect(href).toMatch(/^\/.attachments\//);
  expect(await (await request.get(href ?? "")).text()).toBe("hello file");
  await openNavigation(page);
  await expect(library.getByRole("link", { name: "Download all files", exact: true })).toHaveCount(
    0,
  );
  await library.getByRole("button", { name: "Close navigation", exact: true }).click();
  expect((await request.get("/api/notes/Templates%2FDaily.md")).ok()).toBeTruthy();
  await expect(page.locator("#template")).toHaveCount(0);
});

test("formats text, keeps undo history, and opens notes with the keyboard", async ({ page }) => {
  const textarea = page.getByLabel("Note", { exact: true });
  await textarea.fill("word");
  await selectMarkdown(textarea, 0, 4);
  await textarea.press("Control+b");
  await expect(textarea).toHaveMarkdown("**word**");
  await pressShortcut(textarea, "z");
  await expect(textarea).toHaveMarkdown("word");
  await pressShortcut(textarea, "Shift+Z");
  await expect(textarea).toHaveMarkdown("**word**");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await textarea.press("Control+p");
  const dialog = page.getByRole("dialog", { name: "Open note", exact: true });
  await expect(dialog.getByLabel("Find a note", { exact: true })).toBeFocused();
  await dialog.press("Escape");
  await expect(dialog).not.toBeVisible();
  await expect(textarea).toBeFocused();
});

test("clears a previous note while a new preview is loading", async ({ page, request }) => {
  await request.post("/api/notes", { data: { name: "First.md", body: "First content" } });
  await request.post("/api/notes", { data: { name: "Second.md", body: "Second content" } });
  await page.goto("/?note=First.md");
  await expect(page.getByLabel("Note", { exact: true })).toBeEditable();
  await expect(page.locator("#live-editor")).toContainText("First content");
  let release: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/api/preview", async (route) => {
    if (route.request().postDataJSON().body === "Second content") await held;
    await route.continue();
  });
  try {
    await page.keyboard.press("Control+p");
    await page
      .getByRole("dialog", { name: "Open note", exact: true })
      .getByRole("button", { name: "Second content", exact: true })
      .click();
    await expect(page).toHaveURL(/note=Second.md/);
    await expect(page.locator("#live-editor")).not.toContainText("First content");
  } finally {
    release();
  }
  await expect(page.locator("#live-editor")).toContainText("Second content");
});

test("keeps preview and Trash accessible at narrow widths and enlarged text", async ({
  page,
}, info) => {
  await page
    .getByLabel("Note", { exact: true })
    .fill(
      "# Preview\n\nParagraph with **bold text**.\n\n- [ ] A task\n\n| Column | Column |\n| :--- | ---: |\n| Cell | Cell |\n",
    );
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await expect(page.locator("#live-editor strong")).toHaveText("bold text");
  await expect(page.locator("#live-editor th").first()).toHaveCSS("text-align", "left");
  await expect(page.locator("#live-editor th").last()).toHaveCSS("text-align", "right");
  await page.screenshot({
    path: `../target/screenshots/markdown-${info.project.name}.png`,
    fullPage: true,
  });
  await page.setViewportSize({ width: 320, height: 640 });
  await page.evaluate(() => {
    const root = document.documentElement;
    root.style.fontSize = `${Number.parseFloat(getComputedStyle(root).fontSize) * 2}px`;
  });
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
  ).toBeTruthy();
  expect(await accessibilityViolations(page)).toEqual([]);
  await page.locator("#live-editor").evaluate((element) => {
    const button = document.createElement("button");
    button.id = "unnamed-fixture";
    element.append(button);
  });
  expect(
    (await accessibilityViolations(page)).some((finding) => finding.id === "button-name"),
  ).toBeTruthy();
  await page.locator("#unnamed-fixture").evaluate((element) => element.remove());
  const dialog = await openTrash(page);
  await expect(dialog).toBeVisible();
  const geometry = await dialog.evaluate((element) => {
    const outer = element.getBoundingClientRect();
    return {
      width: element.clientWidth,
      content: element.scrollWidth,
      children: Array.from(element.querySelectorAll("*"))
        .map((child) => {
          const bounds = child.getBoundingClientRect();
          return {
            name: child.id || child.tagName,
            right: bounds.right - outer.left,
            width: bounds.width,
          };
        })
        .filter((child) => child.right > outer.width || child.width > outer.width),
    };
  });
  expect(geometry.content, JSON.stringify(geometry)).toBeLessThanOrEqual(geometry.width);
  expect(await accessibilityViolations(page)).toEqual([]);
  await dialog.press("Escape");
  await expect(page.getByRole("button", { name: "Navigation", exact: true })).toBeFocused();
});

test("follows heading anchors without reopening the saved note", async ({ page }) => {
  await nameNote(page, "Anchor");
  await page.getByLabel("Note", { exact: true }).fill("# Heading\n\n[jump](#heading)");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await pressShortcut(page.getByLabel("Note", { exact: true }), "Home");
  const link = page.locator("#live-editor").getByRole("link", { name: "jump", exact: true });
  await expect(link).toBeVisible();
  let reads = 0;
  page.on("request", (request) => {
    if (request.method() === "GET" && new URL(request.url()).pathname.startsWith("/api/notes/"))
      reads += 1;
  });
  await link.click();
  await expect(page).toHaveURL(/#md-heading$/);
  expect(reads).toBe(0);
  await page.reload();
  await expect(
    page.getByRole("heading", { level: 1, name: "# Heading", exact: true }),
  ).toBeVisible();
  await newNote(page);
  await expect(page).toHaveURL(/\/?\?note=[a-f0-9]{32}\.md$/);
  await page.goBack();
  await expect(
    page.getByRole("heading", { level: 1, name: "# Heading", exact: true }),
  ).toBeVisible();
});
