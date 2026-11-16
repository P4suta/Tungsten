import type { Page } from "@playwright/test";
import { accessibilityViolations } from "./accessibility.js";
import { expect, pressShortcut, test } from "./editor.js";
import { missingIcons } from "./icons-check.js";

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
  await pressShortcut(page.locator(".cm-content"), "End");
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
  for (const name of ["Notes", "Note details"]) {
    await page.getByRole("button", { name, exact: true }).click();
    if (name === "Notes") await expect(paper).toBeHidden();
    else await expect(page.locator("#note-sidebar")).toBeVisible();
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
  const sidebar = page.getByRole("complementary", { name: "Notes", exact: true });
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
  await page.getByRole("button", { name: "New note", exact: true }).click();
  await expect(sidebar).toBeVisible();
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await pressShortcut(editor, "a");
  await page.keyboard.insertText(stationarySource);
  await pressShortcut(editor, "End");
  await expect.poll(() => paper.evaluate((node) => node.scrollTop)).toBeGreaterThan(500);
  expect(await sidebar.boundingBox()).toEqual(bounds);
  expect(await page.evaluate(() => scrollY)).toBe(0);
  expect(await page.evaluate(missingIcons, null)).toEqual([]);
  expect(await accessibilityViolations(page)).toEqual([]);
  await page.screenshot({
    path: `../target/screenshots/writing-navigation-${info.project.name}.png`,
  });
});
