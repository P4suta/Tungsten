import type { Page } from "@playwright/test";
import { accessibilityViolations } from "./accessibility.js";
import { expect, test } from "./editor.js";
import { missingIcons } from "./icons-check.js";
import { openContext } from "./interface.js";

interface PaperPosition {
  left: number;
  top: number;
  width: number;
  height: number;
  contentLeft: number;
  contentWidth: number;
  scrollTop: number;
  selectionStart: number;
  selectionEnd: number;
}

async function paperPosition(page: Page): Promise<PaperPosition> {
  return page.evaluate(() => {
    const paper = document.getElementById("paper-scroll");
    const source = document.getElementById("body");
    const content = document.querySelector(".cm-content");
    if (!(paper instanceof HTMLElement) || !(source instanceof HTMLTextAreaElement) || !content)
      throw new Error("The writing surface is unavailable.");
    const bounds = paper.getBoundingClientRect();
    const writing = content.getBoundingClientRect();
    return {
      left: bounds.left,
      top: bounds.top,
      width: bounds.width,
      height: bounds.height,
      contentLeft: writing.left,
      contentWidth: writing.width,
      scrollTop: paper.scrollTop,
      selectionStart: source.selectionStart,
      selectionEnd: source.selectionEnd,
    };
  });
}

const stationarySource = `# A stationary page\n\n${"Keep a thought where it belongs. Sidebar controls leave the writing page in place. 日本語でも書き続ける。\n\n".repeat(80)}`;

async function centerLastLine(page: Page): Promise<void> {
  await page.locator(".cm-content").press("ControlOrMeta+End");
  await expect
    .poll(() =>
      page.evaluate(() => {
        const paper = document.getElementById("paper-scroll")?.getBoundingClientRect();
        const line = document
          .querySelector(".cm-content")
          ?.lastElementChild?.getBoundingClientRect();
        if (!paper || !line) throw new Error("The writing position is unavailable.");
        return Math.abs(line.top + line.height / 2 - (paper.top + paper.bottom) / 2);
      }),
    )
    .toBeLessThan(2);
}

test("keeps the paper centered and stationary across every wide sidebar combination", async ({
  page,
  request,
}, info) => {
  const name = `Stationary-${info.project.name}-${info.repeatEachIndex}.md`;
  expect((await request.post("/api/notes", { data: { name, body: stationarySource } })).ok()).toBe(
    true,
  );
  for (const width of [1440, 1024, 1280, 1920]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto(`/?note=${name}`);
    const editor = page.locator(".cm-content");
    await expect(editor).toBeEditable();
    const navigation = page.getByRole("button", { name: "Navigation", exact: true });
    const context = page.getByRole("button", { name: "Note context", exact: true });
    for (const control of [navigation, context])
      if ((await control.getAttribute("aria-expanded")) === "true") await control.click();
    await centerLastLine(page);
    await expect
      .poll(() => page.locator("#paper-scroll").evaluate((node) => node.scrollTop))
      .toBeGreaterThan(500);
    const positions = [await paperPosition(page)];
    if (width === 1440)
      await page.screenshot({
        path: `../target/screenshots/stationary-paper-${info.project.name}-closed.png`,
      });
    for (const control of [navigation, context, navigation, context]) {
      await control.click();
      positions.push(await paperPosition(page));
      if (width === 1440 && positions.length === 2)
        await page.screenshot({
          path: `../target/screenshots/stationary-paper-${info.project.name}-navigation.png`,
        });
    }
    for (const position of positions) expect(position).toEqual(positions[0]);
    const reference = positions[0];
    if (!reference) throw new Error("Missing paper position.");
    expect(Math.abs(reference.left + reference.width / 2 - width / 2)).toBeLessThan(1);
    await expect(page.locator("#body")).toHaveValue(stationarySource);
  }
});

test("returns from compact sidebars to the same reading position and selection", async ({
  page,
  request,
}, info) => {
  const name = `Compact-position-${info.project.name}-${info.repeatEachIndex}.md`;
  expect((await request.post("/api/notes", { data: { name, body: stationarySource } })).ok()).toBe(
    true,
  );
  await page.setViewportSize({ width: 390, height: 760 });
  await page.goto(`/?note=${name}`);
  await expect(page.locator(".cm-content")).toBeEditable();
  await centerLastLine(page);
  const paper = page.locator("#paper-scroll");
  await expect.poll(() => paper.evaluate((node) => node.scrollTop)).toBeGreaterThan(500);
  const position = await paperPosition(page);
  for (const name of ["Navigation", "Note context"]) {
    await page.getByRole("button", { name, exact: true }).click();
    await expect(paper).toBeHidden();
    await page.keyboard.press("Escape");
    await expect(paper).toBeVisible();
    await expect.poll(() => paperPosition(page)).toEqual(position);
  }
  await expect(page.locator("#body")).toHaveValue(stationarySource);
});

test("keeps navigation beside the paper through creation, note switching, and scrolling", async ({
  page,
  request,
}, info) => {
  const suffix = `${info.project.name}-${info.repeatEachIndex}`;
  const first = `Docked-first-${suffix}.md`;
  const second = `Docked-second-${suffix}.md`;
  const secondBody = `# Second ${suffix}\n\nAnother note.\n`;
  await page.setViewportSize({ width: 1440, height: 900 });
  for (const [name, body] of [
    [first, `# First ${suffix}\n\nWriting.\n`],
    [second, secondBody],
  ])
    expect((await request.post("/api/notes", { data: { name, body } })).ok()).toBe(true);
  await page.goto(`/?note=${first}`);
  const sidebar = page.getByRole("complementary", { name: "Navigation", exact: true });
  const paper = page.locator("#paper-scroll");
  await expect(sidebar).toBeVisible();
  await expect(sidebar).not.toHaveAttribute("popover");
  const bounds = await sidebar.boundingBox();
  const paperBounds = await paper.boundingBox();
  if (!bounds || !paperBounds) throw new Error("The pane layout is unavailable.");
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(paperBounds.x);
  await sidebar.getByRole("link", { name: `Second ${suffix}`, exact: true }).click();
  await expect(page.getByRole("textbox", { name: "Note", exact: true })).toHaveMarkdown(secondBody);
  await expect(sidebar).toBeVisible();
  expect(await sidebar.boundingBox()).toEqual(bounds);
  await sidebar.getByRole("button", { name: "New note", exact: true }).click();
  await expect(sidebar).toBeVisible();
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.press("ControlOrMeta+a");
  await page.keyboard.insertText(`# A long note\n\n${"A writing line.\n".repeat(100)}`);
  await editor.press("ControlOrMeta+End");
  await expect.poll(() => paper.evaluate((node) => node.scrollTop)).toBeGreaterThan(500);
  expect(await sidebar.boundingBox()).toEqual(bounds);
  expect(await page.evaluate(() => scrollY)).toBe(0);
  expect(await page.evaluate(missingIcons, null)).toEqual([]);
  expect(await accessibilityViolations(page)).toEqual([]);
});

test("follows the current note in context without letting a delayed history read block writing", async ({
  page,
  request,
}, info) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const source = `context-source-${info.project.name}.md`;
  const target = `context-target-${info.project.name}.md`;
  const incoming = `context-incoming-${info.project.name}.md`;
  for (const [name, body] of [
    [source, `# Context source\n\n[Target](${target})\n`],
    [target, "# Context target\n\nWriting.\n"],
    [incoming, `# Context incoming\n\n[Source](${source})\n`],
  ])
    await request.post("/api/notes", { data: { name, body } });
  await page.goto(`/?note=${source}`);
  const context = await openContext(page);
  await expect(
    context.locator("#incoming-notes").getByRole("link", { name: "Context incoming", exact: true }),
  ).toBeVisible();
  await context
    .locator("#outgoing-notes")
    .getByRole("link", { name: "Context target", exact: true })
    .click();
  await expect(page.locator("#body")).toHaveValue(/^# Context target/);
  await expect(
    context.locator("#incoming-notes").getByRole("link", { name: "Context source", exact: true }),
  ).toBeVisible();
  await expect(context.locator("#outgoing-notes a")).toHaveCount(0);
  let release: () => void = () => {};
  let arrived: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const requested = new Promise<void>((resolve) => {
    arrived = resolve;
  });
  await page.route("**/api/history?*", async (route) => {
    if (new URL(route.request().url()).searchParams.get("name") === target) {
      arrived();
      await held;
    }
    await route.continue();
  });
  await context.getByRole("button", { name: "Version history", exact: true }).click();
  await requested;
  try {
    const editor = page.getByRole("textbox", { name: "Note", exact: true });
    await expect(editor).toBeEditable();
    await editor.fill("# Context target\n\nKeep typing while history loads.\n");
    await expect(page.locator("#save-status")).toHaveText("Saved");
    await editor.fill(`# Context target\n\n[Return](${source})\n`);
    await expect(page.locator("#save-status")).toHaveText("Saved");
    await context.getByRole("button", { name: "Links", exact: true }).click();
    await expect(
      context.locator("#outgoing-notes").getByRole("link", { name: "Context source", exact: true }),
    ).toBeVisible();
    await expect(context.locator("#outgoing-notes")).not.toContainText("No links yet.");
    await context
      .locator("#incoming-notes")
      .getByRole("link", { name: "Context source", exact: true })
      .click();
    await expect(page.locator("#body")).toHaveValue(/^# Context source/);
  } finally {
    release();
  }
  await context.getByRole("button", { name: "Version history", exact: true }).click();
  await expect(context.locator("#history-status")).toHaveText(
    "No recovery points from the past 7 days.",
  );
  await expect(context.locator("#history-list button")).toHaveCount(0);
  await expect(context).toBeVisible();
});
