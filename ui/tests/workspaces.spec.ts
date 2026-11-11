import { accessibilityViolations } from "./accessibility.js";
import { expect, test } from "./editor.js";
import { missingIcons } from "./icons-check.js";
import { openNavigation } from "./interface.js";

test("saves named working sets across reloads without changing note links", async ({
  page,
  request,
}, info) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const first = `workspace-first-${info.project.name}.md`;
  const second = `workspace-second-${info.project.name}.md`;
  await request.post("/api/notes", {
    data: { name: first, body: "# Working thought\n\nOriginal text.\n" },
  });
  await request.post("/api/notes", {
    data: { name: second, body: "# Another working thought\n\nSeparate text.\n" },
  });
  await page.goto(`/?note=${first}`);
  const sidebar = await openNavigation(page);
  const group = `Project A ${info.project.name}`;
  await sidebar.getByRole("button", { name: "New workspace", exact: true }).click();
  await sidebar.getByLabel("Workspace name", { exact: true }).fill(group);
  await sidebar.getByRole("button", { name: "Save workspace", exact: true }).click();
  await expect(sidebar.locator("#library-title")).toHaveText(group);
  await expect(sidebar.locator("#sidebar-search").locator("..").locator("svg")).toHaveAttribute(
    "data-icon",
    "search",
  );
  await expect(sidebar.locator("#remove-workspace svg")).toHaveAttribute("data-icon", "close");
  await expect(sidebar.locator("#add-current-note svg")).toHaveAttribute("data-icon", "add");
  await expect(sidebar.locator("#sidebar-notes a")).toHaveCount(0);
  await sidebar.getByRole("button", { name: "Add current note", exact: true }).click();
  const firstLink = sidebar.getByRole("link", { name: "Working thought", exact: true });
  await expect(firstLink).toBeVisible();
  await sidebar.getByLabel("Search all notes", { exact: true }).fill("Another working thought");
  await sidebar
    .getByRole("button", { name: `Add Another working thought to ${group}`, exact: true })
    .click();
  await sidebar.getByLabel("Search all notes", { exact: true }).fill("");
  const secondLink = sidebar.getByRole("link", { name: "Another working thought", exact: true });
  await expect(secondLink).toBeVisible();
  await secondLink.click();
  await expect(page.locator("#body")).toHaveValue(/^# Another working thought/);
  await expect(sidebar.locator("#library-title")).toHaveText(group);
  const row = await firstLink.elementHandle();
  await firstLink.focus();
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect
    .poll(() => firstLink.evaluate((node, original) => node === original, row))
    .toBe(true);
  await expect(firstLink).toBeFocused();
  await page.reload();
  await expect(sidebar.locator("#library-title")).toHaveText(group);
  await expect(firstLink).toBeVisible();
  await expect(secondLink).toBeVisible();
  expect(await page.evaluate(missingIcons, null)).toEqual([]);
  expect(await accessibilityViolations(page)).toEqual([]);
  await sidebar.getByRole("button", { name: "Rename workspace", exact: true }).click();
  await sidebar.getByLabel("Workspace name", { exact: true }).fill(`${group} renamed`);
  await sidebar.getByRole("button", { name: "Save workspace", exact: true }).click();
  await expect(sidebar.locator("#library-title")).toHaveText(`${group} renamed`);
  page.once("dialog", (dialog) => {
    void dialog.accept();
  });
  await sidebar.getByRole("button", { name: "Remove workspace", exact: true }).click();
  await expect(sidebar.locator("#library-title")).toHaveText("All notes");
  const unchanged: { body: string } = await (await request.get(`/api/notes/${first}`)).json();
  expect(unchanged.body).toBe("# Working thought\n\nOriginal text.\n");
  expect((await request.get(`/api/notes/${second}`)).ok()).toBe(true);
});

test("creates a note inside a compact workspace and retries membership without duplicating the note", async ({
  page,
  request,
}, info) => {
  await page.setViewportSize({ width: 390, height: 760 });
  await page.goto("/");
  const sidebar = await openNavigation(page);
  const group = `Compact project ${info.project.name}`;
  await sidebar.getByRole("button", { name: "New workspace", exact: true }).click();
  await sidebar.getByLabel("Workspace name", { exact: true }).fill(group);
  await sidebar.getByRole("button", { name: "Save workspace", exact: true }).click();
  await expect(sidebar.locator("#library-title")).toHaveText(group);
  let reject = true;
  let writes = 0;
  await page.route("**/api/workspaces/*/notes/*", async (route) => {
    if (route.request().method() === "PUT") {
      writes++;
      if (reject) {
        await route.abort();
        return;
      }
    }
    await route.continue();
  });
  await sidebar.getByRole("button", { name: "New note", exact: true }).click();
  await expect(page.locator("#save-status")).toHaveText("Saved");
  await expect(page.getByRole("textbox", { name: "Note", exact: true })).toBeFocused();
  const name = new URL(page.url()).searchParams.get("note");
  expect(name).toBeTruthy();
  await openNavigation(page);
  await expect(sidebar.locator("#library-retry")).toBeVisible();
  reject = false;
  await sidebar.locator("#library-retry").click();
  const members = sidebar.locator("#sidebar-notes a");
  await expect(members).toHaveCount(1);
  expect(
    new URL((await members.first().getAttribute("href")) ?? "", page.url()).searchParams.get(
      "note",
    ),
  ).toBe(name);
  expect(writes).toBe(2);
  await members.first().click();
  await expect(page.locator("#paper-scroll")).toBeVisible();
  expect(new URL(page.url()).searchParams.get("note")).toBe(name);
  expect((await request.get(`/api/notes/${name}`)).ok()).toBe(true);
});

test("shares collections between tabs while keeping each tab's selection independent", async ({
  page,
  context,
  request,
}, info) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const name = `shared-working-set-${info.project.name}.md`;
  await request.post("/api/notes", { data: { name, body: "# Shared working thought\n" } });
  await page.goto(`/?note=${name}`);
  const second = await context.newPage();
  await second.setViewportSize({ width: 1440, height: 900 });
  await second.goto(`/?note=${name}`);
  const sidebar = await openNavigation(page);
  const other = await openNavigation(second);
  const group = `Shared project ${info.project.name}`;
  await sidebar.getByRole("button", { name: "New workspace", exact: true }).click();
  await sidebar.getByLabel("Workspace name", { exact: true }).fill(group);
  await sidebar.getByRole("button", { name: "Save workspace", exact: true }).click();
  await expect(sidebar.locator("#library-title")).toHaveText(group);
  await expect(other.getByRole("button", { name: group, exact: true })).toBeVisible();
  await expect(other.locator("#library-title")).toHaveText("All notes");
  await other.getByRole("button", { name: group, exact: true }).click();
  await other.getByRole("button", { name: "Add current note", exact: true }).click();
  await expect(
    sidebar.getByRole("link", { name: "Shared working thought", exact: true }),
  ).toBeVisible();
  await other.getByRole("button", { name: "All notes", exact: true }).click();
  await expect(sidebar.locator("#library-title")).toHaveText(group);
  await expect(other.locator("#library-title")).toHaveText("All notes");
  await second.close();
});

test("keeps navigation focus when a new note finishes saving in the background", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 760 });
  await page.goto("/");
  let release: () => void = () => {};
  let arrived: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const requested = new Promise<void>((resolve) => {
    arrived = resolve;
  });
  await page.route("**/api/notes", async (route) => {
    if (route.request().method() === "POST") {
      arrived();
      await held;
    }
    await route.continue();
  });
  try {
    await (await openNavigation(page))
      .getByRole("button", { name: "New note", exact: true })
      .click();
    await requested;
    const sidebar = await openNavigation(page);
    await sidebar.getByRole("button", { name: "New workspace", exact: true }).click();
    const name = sidebar.getByLabel("Workspace name", { exact: true });
    await name.fill("Keep naming the workspace");
    release();
    await expect(page.locator("#save-status")).toHaveText("Saved");
    await expect(sidebar).toBeVisible();
    await expect(name).toBeFocused();
    await expect(name).toHaveValue("Keep naming the workspace");
  } finally {
    release();
  }
});
