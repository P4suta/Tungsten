import { expect, replaceMarkdown, test } from "./editor.js";
import { openNavigation } from "./interface.js";

test("reclaims closed navigation space and opens details as a keyboard modal", async ({
  page,
  request,
}, info) => {
  const name = `Workspace-layout-${info.project.name}.md`;
  const text = "# Writing notes\n\nA thought worth keeping.\n";
  expect((await request.post("/api/notes", { data: { name, body: text } })).ok()).toBe(true);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto(`/?note=${name}`);
  await expect(page.locator(".cm-content")).toBeEditable();
  await page.screenshot({ path: `../target/screenshots/web-writing-${info.project.name}.png` });
  const paper = page.locator("#paper-scroll");
  const opened = await paper.boundingBox();
  await page.getByRole("button", { name: "Notes", exact: true }).click();
  const closed = await paper.boundingBox();
  expect(opened).not.toBeNull();
  expect(closed?.width ?? 0).toBeGreaterThan(opened?.width ?? 0);
  expect(closed?.x).toBe(0);
  const trigger = page.getByRole("button", { name: "Note details", exact: true });
  await trigger.click();
  const details = page.getByRole("dialog", { name: "Note details", exact: true });
  await expect(details).toBeVisible();
  await page.screenshot({ path: `../target/screenshots/web-details-${info.project.name}.png` });
  expect(await details.evaluate((node) => node.matches(":modal"))).toBe(true);
  await page.keyboard.press("Shift+Tab");
  await expect(details.getByRole("button", { name: "Delete", exact: true })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(
    details.getByRole("button", { name: "Close note details", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(details).toBeHidden();
  await expect(trigger).toBeFocused();
  await expect(page.locator("#body")).toHaveValue(text);
  await expect(page.locator("#home-graph, #graph-nodes")).toHaveCount(0);
  await expect(page.locator("#save-status")).toBeVisible();
});

test("follows linked notes and dismisses a pending history read before editing", async ({
  page,
  request,
}, info) => {
  const source = `Linked-source-${info.project.name}.md`;
  const target = `Linked-target-${info.project.name}.md`;
  const incoming = `Linked-incoming-${info.project.name}.md`;
  for (const [name, body] of [
    [source, `# Source\n\n[Target](${target})\n`],
    [target, "# Target\n\nWriting.\n"],
    [incoming, `# Incoming\n\n[Source](${source})\n`],
  ])
    expect((await request.post("/api/notes", { data: { name, body } })).ok()).toBe(true);
  await page.goto(`/?note=${source}`);
  const trigger = page.getByRole("button", { name: "Note details", exact: true });
  await trigger.click();
  const details = page.getByRole("dialog", { name: "Note details", exact: true });
  await expect(
    details.locator("#incoming-notes").getByRole("link", { name: "Incoming", exact: true }),
  ).toBeVisible();
  await details
    .locator("#outgoing-notes")
    .getByRole("link", { name: "Target", exact: true })
    .click();
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await expect(editor).toBeFocused();
  await expect(editor).toHaveMarkdown(/^# Target/);
  let release: () => void = () => {};
  let arrived: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const requested = new Promise<void>((resolve) => {
    arrived = resolve;
  });
  await page.route("**/api/history?*", async (route) => {
    arrived();
    await held;
    await route.continue();
  });
  try {
    await trigger.click();
    await details.getByRole("button", { name: "Version history", exact: true }).click();
    await requested;
    await page.keyboard.press("Escape");
    await replaceMarkdown(editor, "# Target\n\nKeep writing while history loads.\n");
    await expect(page.locator("#save-status")).toHaveText("Saved");
  } finally {
    release();
  }
  await expect(details).toBeHidden();
  expect((await (await request.get(`/api/notes/${source}`)).json()).body).toContain(target);
});

test("keeps search before optional organization and writing available at 320 pixels", async ({
  page,
}) => {
  await page.setViewportSize({ width: 320, height: 760 });
  await page.goto("/");
  await expect(page.locator(".cm-content")).toBeEditable();
  await expect(page.getByRole("button", { name: "New note", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Notes", exact: true }).click();
  const search = page.getByLabel("Search all notes", { exact: true });
  await expect(search).toBeVisible();
  const workspaces = page.locator("#workspace-browser");
  await expect(workspaces).not.toHaveAttribute("open");
  await workspaces.locator("summary").click();
  await expect(page.getByRole("button", { name: "New workspace", exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator(".cm-content")).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test("keeps navigation actions reachable with enlarged and spaced text", async ({ page }) => {
  for (const viewport of [
    { width: 320, height: 640 },
    { width: 800, height: 320 },
  ]) {
    await page.setViewportSize(viewport);
    await page.goto("/");
    await expect(page.locator(".cm-content")).toBeEditable();
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
    const trash = sidebar.getByRole("button", { name: "Trash", exact: true });
    await trash.focus();
    await expect(trash).toBeInViewport({ ratio: 1 });
    await trash.click();
    const back = sidebar.getByRole("button", { name: "Back to notes", exact: true });
    await back.focus();
    await expect(back).toBeInViewport({ ratio: 1 });
    await back.click();
    const search = sidebar.getByRole("searchbox", { name: "Search all notes", exact: true });
    await search.focus();
    await expect(search).toBeInViewport({ ratio: 1 });
    await page.keyboard.press("Escape");
    await expect(page.locator(".cm-content")).toBeVisible();
  }
});
