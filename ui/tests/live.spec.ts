import { expect, test } from "@playwright/test";
import { accessibilityViolations } from "./accessibility.js";
import { pressShortcut } from "./editor.js";
import { missingIcons } from "./icons-check.js";
import {
  exploreNotes,
  newEmptyNote,
  newNote,
  openNavigation,
  openTrash,
  showPaper,
} from "./interface.js";

test("keeps an editable dated title and functional note exploration", async ({ page }, info) => {
  await page.goto("/");
  await newNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await expect(page.getByRole("button", { name: "Navigation", exact: true })).toBeVisible();
  await expect(page.locator("header")).not.toContainText("Tungsten");
  await expect(page.locator(".title-cue,.body-cue")).toHaveCount(0);
  await page.screenshot({
    path: `../target/screenshots/live-blank-${info.project.name}.png`,
    fullPage: true,
  });
  expect(
    await page.evaluate(() => document.documentElement.scrollHeight <= innerHeight + 1),
  ).toBeTruthy();
  const skip = page.getByRole("link", { name: "Skip to note", exact: true });
  await skip.focus();
  await skip.press("Enter");
  await expect(editor).toBeFocused();
  await pressShortcut(editor, "a");
  await page.keyboard.insertText("# A title\n\nThe first paragraph.");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await page.screenshot({
    path: `../target/screenshots/live-${info.project.name}.png`,
    fullPage: true,
  });
  await exploreNotes(page);
  await expect(page.getByRole("dialog", { name: "Explore notes", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Explore A title", exact: true })).toBeVisible();
  await expect(
    page.locator("#folder-filter, #tag-filter, #name-form, #pin, #template"),
  ).toHaveCount(0);
});

test("renders inactive Markdown and reveals the selected source without losing edits", async ({
  page,
  request,
}, info) => {
  const name = `live-${info.project.name}.md`;
  await request.post("/api/notes", {
    data: { name, body: "# Live note\n\nA **bold** paragraph.\n\nLast line." },
  });
  await page.goto(`/?note=${encodeURIComponent(name)}`);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await expect(editor).toBeVisible();
  await pressShortcut(editor, "End");
  await expect(page.locator(".cm-content strong")).toHaveText("bold");
  await page.locator(".cm-content strong").click();
  await expect(page.locator(".cm-line").filter({ hasText: "A **bold** paragraph." })).toBeVisible();
  await pressShortcut(editor, "End");
  await editor.pressSequentially(" More.");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const saved = await request.get(`/api/notes/${encodeURIComponent(name)}`);
  expect((await saved.json()).body).toBe(
    "# Live note\n\nA **bold** paragraph.\n\nLast line. More.",
  );
  expect(await accessibilityViolations(page)).toEqual([]);
});

test("formats Markdown automatically after editing and supports a single undo", async ({
  page,
  request,
}, info) => {
  const name = `format-${info.project.name}.md`;
  const source = "# Format\n\n*   first\n*   second\n\n```txt\n  keep   this\n```\n";
  await request.post("/api/notes", { data: { name, body: source } });
  await page.goto(`/?note=${encodeURIComponent(name)}`);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.click();
  await pressShortcut(editor, "End");
  await editor.press("Alt+Shift+F");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await expect
    .poll(
      async () => (await (await request.get(`/api/notes/${encodeURIComponent(name)}`)).json()).body,
    )
    .toContain("- first\n- second");
  expect(
    (await (await request.get(`/api/notes/${encodeURIComponent(name)}`)).json()).body,
  ).toContain("  keep   this");
  await pressShortcut(editor, "z");
  await expect
    .poll(
      async () => (await (await request.get(`/api/notes/${encodeURIComponent(name)}`)).json()).body,
    )
    .toBe(source);
  const more = await openNavigation(page);
  await expect(more.getByRole("button", { name: "Format Markdown", exact: true })).toHaveCount(0);
});

test("keeps storage identities stable across title edits and duplicate titles", async ({
  page,
  request,
}) => {
  await page.goto("/");
  for (let index = 0; index < 2; index++) {
    await newEmptyNote(page);
    const editor = page.getByRole("textbox", { name: "Note", exact: true });
    await editor.pressSequentially("Shared title");
    await editor.press("Enter");
    await editor.pressSequentially(`Body ${index}`);
    await expect(page.locator("#save-status")).toHaveText("Saved");
  }
  const address = new URL(page.url());
  const identity = address.searchParams.get("note");
  expect(identity).toMatch(/^[a-f0-9]{32}\.md$/);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await pressShortcut(editor, "Home");
  await editor.press("Home");
  await editor.press("Shift+End");
  await page.keyboard.insertText("# Renamed title");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  expect(new URL(page.url()).searchParams.get("note")).toBe(identity);
  const saved = await request.get(`/api/notes/${encodeURIComponent(identity ?? "")}`);
  expect((await saved.json()).body).toContain("# Renamed title");
  const more = await openNavigation(page);
  await expect(more).not.toContainText(identity ?? "unreachable identifier");
});

test("requires icons on controls throughout navigation and rejects missing icons", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByRole("textbox", { name: "Note", exact: true })).toBeVisible();
  expect(await page.evaluate(missingIcons, null)).toEqual([]);
  const more = await openNavigation(page);
  expect(await page.evaluate(missingIcons, null)).toEqual([]);
  await more.getByRole("button", { name: "Close navigation", exact: true }).click();
  const settings = await openTrash(page);
  expect(await page.evaluate(missingIcons, null)).toEqual([]);
  await settings.getByRole("button", { name: "Back to notes", exact: true }).click();
  await exploreNotes(page);
  expect(await page.evaluate(missingIcons, null)).toEqual([]);
  expect(
    await page.evaluate<string[], "missing" | "valid" | null>(missingIcons, "missing"),
  ).toEqual(["missing-icon-fixture"]);
  expect(await page.evaluate<string[], "missing" | "valid" | null>(missingIcons, "valid")).toEqual(
    [],
  );
});

test("inserts and displays an original image with a live slash command", async ({
  page,
  request,
}) => {
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.pressSequentially("An image");
  await editor.press("Enter");
  await editor.pressSequentially("/file");
  await expect(page.getByRole("option", { name: "File /file", exact: true })).toBeVisible();
  const choosing = page.waitForEvent("filechooser");
  await editor.press("Enter");
  const bytes = Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jVfoAAAAASUVORK5CYII=",
    "base64",
  );
  await (await choosing).setFiles({
    name: "live-original.png",
    mimeType: "image/png",
    buffer: bytes,
  });
  await expect(page.locator("#body")).toHaveValue(/!\[live-original\.png\]\(.+\)/);
  await editor.press("End");
  await editor.press("Enter");
  await editor.press("Enter");
  await editor.pressSequentially("Below the image");
  await expect(page.locator(".live-block img[alt='live-original.png']")).toBeVisible();
  expect(
    await page
      .locator(".live-block img")
      .evaluate((node: HTMLImageElement) => node.complete && node.naturalWidth > 0),
  ).toBe(true);
  const id = /\.attachments\/([a-f0-9]{32}\.png)/.exec(
    await page.locator("#body").inputValue(),
  )?.[1];
  expect(id).toBeDefined();
  expect(await (await request.get(`/.attachments/${id}`)).body()).toEqual(bytes);
  expect(await page.evaluate(missingIcons, null)).toEqual([]);
});

test("centers live typing across long paragraphs and wrapping", async ({ page }) => {
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  for (const width of [1280, 320]) {
    await page.setViewportSize({ width, height: 700 });
    await pressShortcut(editor, "a");
    await page.keyboard.insertText(`# Long note\n\n${"長い文章を続けて書く。".repeat(100)}`);
    await editor.pressSequentially("typing");
    await expect
      .poll(() =>
        page.evaluate(() => {
          const selection = getSelection();
          if (!selection?.rangeCount) return 100;
          const range = selection.getRangeAt(0).cloneRange();
          let bounds = range.getBoundingClientRect();
          if (!bounds.height && range.endOffset) {
            range.setStart(range.endContainer, range.endOffset - 1);
            bounds = range.getBoundingClientRect();
          }
          const center =
            (visualViewport?.offsetTop ?? 0) + (visualViewport?.height ?? innerHeight) / 2;
          return Math.abs(bounds.top + bounds.height / 2 - center);
        }),
      )
      .toBeLessThan(40);
  }
});

test("protects Japanese composition and paired input in live editing", async ({
  page,
  request,
}) => {
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  let writes = 0;
  page.on("request", (outgoing) => {
    if (outgoing.method() === "POST" && new URL(outgoing.url()).pathname === "/api/notes") writes++;
  });
  await page.clock.install();
  await page.evaluate(() => {
    document.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    const source = document.querySelector<HTMLTextAreaElement>("#body");
    if (!source) throw new Error("Missing source adapter");
    source.value = "# 日本語の変換中\n\n";
    source.setSelectionRange(source.value.length - 2, source.value.length - 2);
    source.dispatchEvent(
      new InputEvent("input", {
        bubbles: true,
        inputType: "insertCompositionText",
        data: "日本語の変換中",
        isComposing: true,
      }),
    );
  });
  await page.clock.fastForward(1200);
  expect(writes).toBe(0);
  await page.evaluate(() =>
    document.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true })),
  );
  await page.clock.fastForward(1200);
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const currentName = new URL(page.url()).searchParams.get("note");
  expect(
    (await (await request.get(`/api/notes/${encodeURIComponent(currentName ?? "")}`)).json()).body,
  ).toContain("日本語の変換中");
  await editor.press("Enter");
  await page.clock.fastForward(100);
  await editor.pressSequentially("*");
  await expect(page.locator("#body")).toHaveValue("# 日本語の変換中\n**\n\n");
  await editor.press("Backspace");
  await expect(page.locator("#body")).not.toHaveValue(/\*\*/);
});

test("automatically formats after blur and refuses stale formatting results", async ({
  page,
  request,
}, info) => {
  const name = `blur-${info.project.name}.md`;
  await request.post("/api/notes", { data: { name, body: "# Blur\n\n*   first" } });
  await page.goto(`/?note=${encodeURIComponent(name)}`);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.click();
  await pressShortcut(editor, "End");
  await editor.pressSequentially(" item");
  const more = await openNavigation(page);
  await expect
    .poll(
      async () => (await (await request.get(`/api/notes/${encodeURIComponent(name)}`)).json()).body,
    )
    .toContain("- first item");
  await more.getByRole("button", { name: "Close navigation", exact: true }).click();
  await editor.fill("# Blur\n\n*   selected");
  await expect(page.locator("#body")).toHaveValue("# Blur\n\n*   selected");
  await editor.evaluate((node) => {
    (node as HTMLElement).blur();
    (node as HTMLElement).focus();
    document.getSelection()?.selectAllChildren(node);
  });
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => resolve())));
  await expect(page.locator("#body")).toHaveValue("# Blur\n\n*   selected");
  await page.evaluate(() => {
    const source = document.querySelector<HTMLTextAreaElement>("#body");
    const live = document.querySelector<HTMLElement>(".cm-content");
    if (!source || !live) throw new Error("Missing editor");
    source.value = "# Blur\n\n*   latest";
    source.dispatchEvent(new Event("input", { bubbles: true }));
    live.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "F",
        altKey: true,
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    source.value += "\n\nNewer input";
    source.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await expect(page.locator("#body")).toHaveValue("# Blur\n\n*   latest\n\nNewer input");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  expect((await (await request.get(`/api/notes/${encodeURIComponent(name)}`)).json()).body).toBe(
    "# Blur\n\n*   latest\n\nNewer input",
  );
});

test("allows browser text enlargement without changing fixed writing behavior or object icons", async ({
  page,
}) => {
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.pressSequentially("Home");
  await editor.press("Enter");
  await editor.pressSequentially("Body");
  await editor.press("Enter");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await exploreNotes(page);
  await expect(page.locator(".graph-node svg").first()).toHaveAttribute("data-icon", "note");
  await page
    .getByRole("dialog", { name: "Explore notes", exact: true })
    .getByRole("button", { name: "Close", exact: true })
    .click();
  await showPaper(page);
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "225%";
  });
  await expect(page.locator(".cm-editor")).toHaveCSS("font-size", "36px");
  await expect(editor).toHaveAttribute("spellcheck", "true");
  await pressShortcut(editor, "End");
  await editor.press("ArrowUp");
  await editor.press("Home");
  await editor.press("Tab");
  await expect(page.locator("#body")).toHaveValue(/\n {4}Body/);
});
