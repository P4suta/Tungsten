import { expect, test } from "@playwright/test";
import { pressShortcut, selectAllMarkdown } from "./editor.js";
import { newEmptyNote } from "./interface.js";

test("clears the complete Markdown source and restores it through native undo", async ({
  page,
}) => {
  await page.goto("/");
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  const original = "# Whole selection\n\n**Bold words** and `literal text`";
  await editor.fill(original);
  await expect(page.locator("#save-status")).toHaveText("Saved");
  for (const key of ["Backspace", "Delete"]) {
    await selectAllMarkdown(editor);
    await editor.press(key);
    await expect(page.locator("#body")).toHaveValue("");
    await pressShortcut(editor, "z");
    await expect(page.locator("#body")).toHaveValue(original);
  }
});

test("keeps exactly the heading the owner types in an empty note", async ({ page }) => {
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.pressSequentially("# Title");
  await expect(page.locator("#body")).toHaveValue("# Title");
  await editor.press("Enter");
  await editor.pressSequentially("Body");
  await expect(page.locator("#body")).toHaveValue("# Title\nBody");
  await pressShortcut(editor, "a");
  await editor.press("Backspace");
  await expect(page.locator("#body")).toHaveValue("");
  await editor.pressSequentially("# Again");
  await expect(page.locator("#body")).toHaveValue("# Again");
});

test("styles neighboring emphasis while editing an inline expression", async ({ page }, info) => {
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await pressShortcut(editor, "a");
  await page.keyboard.insertText("# Format\n\n**bold** and *italic* and `code` end");
  await expect(page.locator(".cm-content strong")).toHaveText("bold");
  await expect(page.locator(".cm-content em")).toHaveText("italic");
  await expect(page.locator(".cm-content code")).toHaveText("code");
  await page.screenshot({ path: `../target/screenshots/editing-${info.project.name}.png` });
  await editor.press("Home");
  await editor.press("ArrowRight");
  await expect(page.locator(".cm-line").last()).toContainText("**bold**");
  await expect(page.locator(".cm-content em")).toHaveText("italic");
  await editor.pressSequentially("X");
  await expect(page.locator("#body")).toHaveValue(
    "# Format\n\n**bXold** and *italic* and `code` end",
  );
});

test("keeps a list line at the same height while moving the caret", async ({ page }) => {
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await pressShortcut(editor, "a");
  await page.keyboard.insertText("# List\n\n- one\n- two\n- three\n\nEnd");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const positions = await page
    .locator(".cm-content")
    .evaluate((node) =>
      Array.from(node.querySelectorAll(".live-list-line")).map(
        (line) =>
          line.getBoundingClientRect().top +
          (document.querySelector("#paper-scroll")?.scrollTop ?? 0),
      ),
    );
  expect(positions).toHaveLength(3);
  for (let index = 0; index < 5; index++) {
    await editor.press("ArrowUp");
    await expect
      .poll(async () =>
        page
          .locator(".cm-content")
          .evaluate((node) =>
            Array.from(node.querySelectorAll(".live-list-line")).map(
              (line) =>
                line.getBoundingClientRect().top +
                (document.querySelector("#paper-scroll")?.scrollTop ?? 0),
            ),
          ),
      )
      .toEqual(positions);
  }
});

test("uses Tab for link fields and paired delimiters and supports line commands", async ({
  page,
}) => {
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await pressShortcut(editor, "a");
  await page.keyboard.insertText("# Keys\n\n");
  await editor.pressSequentially("[Label");
  await editor.press("Tab");
  await editor.pressSequentially("https://example.com");
  await editor.press("Tab");
  await editor.pressSequentially(" *italic");
  await editor.press("Tab");
  await editor.pressSequentially(" done");
  await expect(page.locator("#body")).toHaveValue(
    "# Keys\n\n[Label](https://example.com) *italic* done",
  );
  await editor.press("Enter");
  await editor.pressSequentially("last");
  await editor.press("Alt+ArrowUp");
  await expect(page.locator("#body")).toHaveValue(
    "# Keys\n\nlast\n[Label](https://example.com) *italic* done",
  );
  await editor.press("Alt+Shift+ArrowDown");
  await expect(page.locator("#body")).toHaveValue(
    "# Keys\n\nlast\nlast\n[Label](https://example.com) *italic* done",
  );
  await pressShortcut(editor, "Shift+K");
  await expect(page.locator("#body")).toHaveValue(
    "# Keys\n\nlast\n[Label](https://example.com) *italic* done",
  );
  await pressShortcut(editor, "z");
  await expect(page.locator("#body")).toHaveValue(
    "# Keys\n\nlast\nlast\n[Label](https://example.com) *italic* done",
  );
});

test("keeps the live cursor independent of queued backing-field selection events", async ({
  page,
}) => {
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.pressSequentially("# Title");
  await page.evaluate(() => {
    const source = document.querySelector<HTMLTextAreaElement>("#body");
    if (!source) throw new Error("Missing source adapter");
    source.setSelectionRange(0, 0);
    source.dispatchEvent(new Event("select"));
  });
  await editor.pressSequentially(" at the end");
  await expect(page.locator("#body")).toHaveValue("# Title at the end");
});

test("offers one unrestricted file command", async ({ page }) => {
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await pressShortcut(editor, "a");
  await page.keyboard.insertText("# Files\n\n/");
  await expect(page.getByRole("option", { name: "File /file", exact: true })).toBeVisible();
  for (const name of ["Image /image", "Audio /audio", "Video /video", "PDF /pdf"])
    await expect(page.getByRole("option", { name, exact: true })).toHaveCount(0);
  await editor.pressSequentially("file");
  const choosing = page.waitForEvent("filechooser");
  await editor.press("Enter");
  const chooser = await choosing;
  expect(await chooser.element().getAttribute("accept")).toBe("");
});

test("keeps keyboard editing available after switching notes", async ({ page, request }, info) => {
  await request.post("/api/notes", {
    data: { name: `keyboard-target-${info.project.name}.md`, body: "# Switch destination" },
  });
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.pressSequentially("# First");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await pressShortcut(editor, "p");
  await page.locator("#quick-search").fill("Switch destination");
  await page
    .locator("#quick-results")
    .getByRole("button", { name: "Switch destination", exact: true })
    .first()
    .click();
  await expect(page.locator("#body")).toHaveValue("# Switch destination");
  await pressShortcut(page, "End");
  await page.keyboard.insertText(" continued");
  await expect(page.locator("#body")).toHaveValue("# Switch destination continued");
});

test("removes results from the previous query while note search is pending", async ({
  page,
  request,
}, info) => {
  const title = `Pending selection ${info.project.name}`;
  const body = `# ${title}`;
  const response = await request.post("/api/notes", {
    data: { name: `pending-selection-${info.project.name}.md`, body },
  });
  expect(response.status()).toBe(201);
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await pressShortcut(editor, "p");
  const results = page.locator("#quick-results");
  const choice = results.getByRole("button", { name: title, exact: true });
  await expect(choice).toBeVisible();
  const held = Promise.withResolvers<void>();
  const arrived = Promise.withResolvers<void>();
  await page.route(
    (url) => url.pathname === "/api/notes" && url.searchParams.get("q") === title.toLowerCase(),
    async (route) => {
      const response = await route.fetch();
      arrived.resolve();
      await held.promise;
      await route.fulfill({ response });
    },
  );
  try {
    await page.locator("#quick-search").fill(title);
    await arrived.promise;
    await expect(choice).toBeHidden();
    await expect(results).toHaveAttribute("aria-busy", "true");
    held.resolve();
    await expect(choice).toBeVisible();
    await expect(results).toHaveAttribute("aria-busy", "false");
    await choice.click();
    await expect(page.getByRole("dialog", { name: "Open note", exact: true })).toBeHidden();
    await expect(page.locator("#body")).toHaveValue(body);
    await pressShortcut(page, "End");
    await page.keyboard.insertText(" continued");
    await expect(page.locator("#body")).toHaveValue(`${body} continued`);
  } finally {
    held.resolve();
  }
});

test("clears pending note results when a search request fails", async ({ page, request }, info) => {
  const title = `Search recovery ${info.project.name}`;
  const response = await request.post("/api/notes", {
    data: { name: `search-recovery-${info.project.name}.md`, body: `# ${title}` },
  });
  expect(response.status()).toBe(201);
  await page.goto("/");
  await newEmptyNote(page);
  await pressShortcut(page.getByRole("textbox", { name: "Note", exact: true }), "p");
  const search = page.locator("#quick-search");
  const results = page.locator("#quick-results");
  await expect(results.getByRole("button", { name: title, exact: true })).toBeVisible();
  await page.route(
    (url) => url.pathname === "/api/notes" && url.searchParams.get("q") === "unavailable",
    (route) => route.abort(),
  );
  const failed = page.waitForEvent("requestfailed", {
    predicate: (request) => new URL(request.url()).searchParams.get("q") === "unavailable",
  });
  await search.fill("Unavailable");
  await failed;
  await expect(results).toHaveAttribute("aria-busy", "false");
  await expect(results.getByRole("button")).toHaveCount(0);
  await expect(search).toBeFocused();
  await search.fill(title);
  await results.getByRole("button", { name: title, exact: true }).click();
  await expect(page.locator("#body")).toHaveValue(`# ${title}`);
});

test("keeps the selected note button during an unchanged search refresh", async ({
  page,
  request,
}, info) => {
  const title = `Stable selection ${info.project.name}`;
  const response = await request.post("/api/notes", {
    data: { name: `stable-selection-${info.project.name}.md`, body: `# ${title}` },
  });
  expect(response.status()).toBe(201);
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await page.keyboard.insertText("# Before selecting");
  await expect(page.locator("#body")).toHaveValue("# Before selecting");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const held = Promise.withResolvers<void>();
  const arrived = Promise.withResolvers<void>();
  await page.route(
    (url) => url.pathname === "/api/notes" && url.search === "",
    async (route) => {
      if (route.request().method() !== "GET") return route.continue();
      const response = await route.fetch();
      arrived.resolve();
      await held.promise;
      await route.fulfill({ response });
    },
  );
  let pressed = false;
  try {
    await pressShortcut(editor, "p");
    await arrived.promise;
    const results = page.locator("#quick-results");
    const choice = results.getByRole("button", { name: title, exact: true });
    await expect(choice).toBeVisible();
    const original = await choice.elementHandle();
    expect(original).not.toBeNull();
    const point = await choice.boundingBox();
    if (!point || !original) throw new Error("Missing note result");
    await page.mouse.move(point.x + point.width / 2, point.y + point.height / 2);
    await page.mouse.down();
    pressed = true;
    const refreshed = page.waitForResponse((response) => {
      const url = new URL(response.url());
      return url.pathname === "/api/notes" && url.searchParams.get("titles") === "true";
    });
    held.resolve();
    await refreshed;
    await expect(results).toHaveAttribute("aria-busy", "false");
    expect(await original.evaluate((node) => node.isConnected)).toBe(true);
    await page.mouse.up();
    pressed = false;
    await expect(page.getByRole("dialog", { name: "Open note", exact: true })).toBeHidden();
    await expect(page.locator("#body")).toHaveValue(`# ${title}`);
  } finally {
    held.resolve();
    if (pressed) await page.mouse.up();
  }
});

test("keeps loading inert and preserves a note started during the initial request", async ({
  page,
  request,
}) => {
  await request.post("/api/notes", { data: { name: "pending-startup.md", body: "# Older note" } });
  const loading = Promise.withResolvers<void>();
  await page.route(
    (url) => url.pathname === "/api/notes",
    async (route) => {
      if (route.request().method() === "GET") await loading.promise;
      await route.continue();
    },
  );
  await page.goto("/");
  await expect(page.locator("#live-editor")).toHaveAttribute("inert", "");
  await page.keyboard.insertText("Unowned input");
  await expect(page.locator("#body")).toHaveValue("");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.pressSequentially("# My new note");
  loading.resolve();
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await expect(page.locator("#body")).toHaveValue("# My new note");
});
