import { accessibilityViolations } from "./accessibility.js";
import { expect, test } from "./editor.js";
import { missingIcons } from "./icons-check.js";
import { exploreNotes, openContext, openNavigation, openTrash, showPaper } from "./interface.js";

test("keeps independently collapsible sidebars beside a pending draft", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  const sidebar = await openNavigation(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  let release: () => void = () => {};
  let arrived: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const requested = new Promise<void>((resolve) => {
    arrived = resolve;
  });
  await page.route("**/api/notes**", async (route) => {
    if (["POST", "PUT"].includes(route.request().method())) {
      arrived();
      await held;
    }
    await route.continue();
  });
  try {
    await editor.fill("# A writing session\n\nKeep writing beside navigation.\n");
    await requested;
    const context = await openContext(page);
    await expect(sidebar).toBeVisible();
    await expect(editor).toBeVisible();
    await expect(context.getByRole("button", { name: "Delete", exact: true })).toBeDisabled();
    await context.getByRole("button", { name: "Close note context", exact: true }).click();
    await expect(sidebar).toBeVisible();
  } finally {
    release();
  }
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await sidebar.getByRole("button", { name: "Close navigation", exact: true }).click();
  await expect(sidebar).toBeHidden();
  await expect(page.getByRole("button", { name: "Navigation", exact: true })).toBeFocused();
  await page.reload();
  await expect(sidebar).toBeHidden();
  await openNavigation(page);
  await page.keyboard.press("Escape");
  await expect(sidebar).toBeHidden();
  await expect(editor).toHaveMarkdown("# A writing session\n\nKeep writing beside navigation.\n");
});

test("switches compact panes without obscuring focus or losing a draft", async ({ page }, info) => {
  await page.setViewportSize({ width: 390, height: 760 });
  await page.goto("/");
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.fill("# Keep this note\n\nA compact writing session.\n");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const sidebar = await openNavigation(page);
  await expect(page.locator("#paper-scroll")).toBeHidden();
  await sidebar.getByRole("button", { name: "Close navigation", exact: true }).press("Shift+Tab");
  await expect(sidebar).toBeVisible();
  expect(
    await page.evaluate(
      () =>
        document.activeElement instanceof HTMLElement &&
        document.activeElement.checkVisibility() &&
        !document.querySelector("#paper-scroll")?.contains(document.activeElement),
    ),
  ).toBe(true);
  const trash = await openTrash(page);
  await expect(trash).toBeVisible();
  await trash.getByRole("button", { name: "Back to notes", exact: true }).click();
  await expect(sidebar.getByRole("region", { name: "Notes library", exact: true })).toBeVisible();
  expect(await page.evaluate(missingIcons, null)).toEqual([]);
  expect(await accessibilityViolations(page)).toEqual([]);
  await page.screenshot({ path: `../target/screenshots/sidebar-${info.project.name}.png` });
  await showPaper(page);
  await expect(editor).toBeFocused();
  await expect(editor).toHaveMarkdown("# Keep this note\n\nA compact writing session.\n");
  await openNavigation(page);
  const skip = page.getByRole("link", { name: "Skip to note", exact: true });
  await skip.focus();
  await skip.press("Enter");
  await expect(editor).toBeFocused();
});

test("returns from exploration to its persistent navigation control", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 760 });
  await page.goto("/");
  await exploreNotes(page);
  await page.getByRole("dialog", { name: "Explore notes", exact: true }).press("Escape");
  const sidebar = page.getByRole("complementary", { name: "Navigation", exact: true });
  await expect(sidebar).toBeVisible();
  await expect(sidebar.getByRole("button", { name: "Explore notes", exact: true })).toBeFocused();
});

test("scrolls the paper independently of navigation and note context", async ({
  page,
  request,
}, info) => {
  for (let index = 0; index < 55; index++)
    await request.post("/api/notes", {
      data: {
        name: `sidebar-scroll-${info.project.name}-${info.repeatEachIndex}-${index}.md`,
        body: `# Shelf thought ${index}\n`,
      },
    });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  const sidebar = await openNavigation(page);
  const context = await openContext(page);
  await expect(sidebar.locator("#sidebar-notes a")).toHaveCount(50);
  await sidebar.evaluate((node) => {
    node.scrollTop = 80;
  });
  const sidebarScroll = await sidebar.evaluate((node) => node.scrollTop);
  expect(sidebarScroll).toBe(80);
  const bounds = await sidebar.boundingBox();
  const rightBounds = await context.boundingBox();
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  const source = `# A long note\n\n${"- A short line\n".repeat(80)}`;
  await editor.fill(source);
  await editor.press("ControlOrMeta+End");
  const paper = page.locator("#paper-scroll");
  await expect.poll(() => paper.evaluate((node) => node.scrollTop)).toBeGreaterThan(500);
  const scroll = await page.evaluate(() => ({
    window: scrollY,
    elements: ["html", "body", "header", "#app-shell", "#paper-scroll", "#editor-pane"].map(
      (selector) => {
        const node = document.querySelector<HTMLElement>(selector);
        return {
          selector,
          bounds: node?.getBoundingClientRect().toJSON(),
          scroll: node?.scrollHeight,
          overflow: node ? getComputedStyle(node).overflow : "",
        };
      },
    ),
  }));
  expect(scroll.window, JSON.stringify(scroll)).toBe(0);
  expect(await sidebar.boundingBox()).toEqual(bounds);
  expect(await context.boundingBox()).toEqual(rightBounds);
  expect(await sidebar.evaluate((node) => node.scrollTop)).toBe(sidebarScroll);
  expect(await page.evaluate(() => scrollY)).toBe(0);
  await expect(editor).toHaveMarkdown(source);
});

test("keeps panes usable through reflow, spacing changes, and unavailable storage", async ({
  page,
}) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, "localStorage", {
      get() {
        throw new DOMException("Storage is unavailable.", "SecurityError");
      },
    });
  });
  await page.goto("/");
  for (const viewport of [
    { width: 1440, height: 900 },
    { width: 320, height: 700 },
    { width: 800, height: 320 },
  ]) {
    await page.setViewportSize(viewport);
    const sidebar = await openNavigation(page);
    await expect
      .poll(() =>
        sidebar.evaluate((node) => {
          const bounds = node.getBoundingClientRect();
          return bounds.left >= 0 && bounds.right <= innerWidth && bounds.height <= innerHeight;
        }),
      )
      .toBe(true);
    await sidebar.getByRole("button", { name: "Close navigation", exact: true }).click();
    await expect(sidebar).toBeHidden();
  }
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "225%";
    const style = document.createElement("style");
    style.nonce =
      document.querySelector<HTMLMetaElement>('meta[name="style-nonce"]')?.content ?? "";
    style.textContent =
      "* {line-height:1.5!important;letter-spacing:.12em!important;word-spacing:.16em!important} p {margin-bottom:2em!important}";
    document.head.append(style);
  });
  const sidebar = await openNavigation(page);
  await expect(sidebar.getByRole("button", { name: "Trash", exact: true })).toBeVisible();
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
    .toBe(true);
  expect(await accessibilityViolations(page)).toEqual([]);
});
