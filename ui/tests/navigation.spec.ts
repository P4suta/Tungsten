import { expect, test } from "@playwright/test";
import { accessibilityViolations } from "./accessibility.js";

test("explores a topic through a movable graph instead of a complete note list", async ({
  page,
  request,
}, info) => {
  for (const [name, body] of [
    ["Philosophy.md", "# Philosophy\n\n[Ethics](Ethics.md)"],
    ["Ethics.md", "# Ethics\n\n[Moral questions](Questions.md)"],
    ["Questions.md", "# Moral questions\n\nHow to live?"],
    ["Unlinked.md", "# Separate thought\n\nA distant idea"],
  ])
    await request.post("/api/notes", { data: { name, body } });
  await page.goto("/?note=Philosophy.md");
  await page.getByRole("link", { name: "Explore notes", exact: true }).click();
  const graph = page.getByRole("region", { name: "Note graph", exact: true });
  await expect(graph).toBeVisible();
  await expect(page.locator("#notes")).toBeHidden();
  await graph.getByRole("button", { name: "Explore Ethics", exact: true }).click();
  await expect(page.locator("#graph-selection")).toContainText("Ethics");
  await expect(
    graph.getByRole("button", { name: "Explore Moral questions", exact: true }),
  ).toBeVisible();
  const before = await graph.getAttribute("data-scale");
  await page.getByRole("button", { name: "Zoom in", exact: true }).click();
  expect(await graph.getAttribute("data-scale")).not.toBe(before);
  const node = graph.getByRole("button", { name: "Explore Ethics", exact: true });
  await node.focus();
  const left = await node.evaluate((button: HTMLElement) => button.style.left);
  await node.press("Shift+ArrowRight");
  expect(await node.evaluate((button: HTMLElement) => button.style.left)).not.toBe(left);
  const nodeBox = await node.boundingBox();
  if (!nodeBox) throw new Error("Missing note bounds");
  const moved = await node.evaluate((button: HTMLElement) => button.style.left);
  await page.mouse.move(nodeBox.x + nodeBox.width / 2, nodeBox.y + nodeBox.height / 2);
  await page.mouse.down();
  await page.mouse.move(nodeBox.x + nodeBox.width / 2 + 20, nodeBox.y + nodeBox.height / 2 + 15);
  await page.mouse.up();
  await expect
    .poll(() => node.evaluate((button: HTMLElement) => button.style.left))
    .not.toBe(moved);
  const pan = await graph.getAttribute("data-pan");
  const box = await graph.boundingBox();
  if (!box) throw new Error("Missing graph bounds");
  await page.mouse.move(box.x + 10, box.y + 10);
  await page.mouse.down();
  await page.mouse.move(box.x + 45, box.y + 35);
  await page.mouse.up();
  await expect.poll(() => graph.getAttribute("data-pan")).not.toBe(pan);
  await page.getByRole("button", { name: "Fit graph", exact: true }).click();
  expect(
    await graph.evaluate((region) => {
      const bounds = region.getBoundingClientRect();
      return Array.from(region.querySelectorAll("button")).every((button) => {
        const node = button.getBoundingClientRect();
        return (
          node.left >= bounds.left &&
          node.right <= bounds.right &&
          node.top >= bounds.top &&
          node.bottom <= bounds.bottom
        );
      });
    }),
  ).toBe(true);
  await page.screenshot({
    path: `../target/screenshots/note-graph-${info.project.name}.png`,
    fullPage: true,
  });
  expect(await accessibilityViolations(page)).toEqual([]);
  await page.getByRole("link", { name: "Open note", exact: true }).click();
  await expect(page).toHaveURL(/note=Ethics.md/);
});

test("leaves an untouched initial timestamp unsaved across background save triggers", async ({
  page,
}) => {
  let writes = 0;
  await page.route(/\/api\/notes(?:\?.*)?$/, async (route) => {
    if (route.request().method() === "GET") await route.fulfill({ json: [] });
    else {
      writes++;
      await route.continue();
    }
  });
  await page.clock.install();
  await page.goto("/");
  await expect(page.locator("#body")).toHaveValue(/^# \d{4}-/);
  await page.evaluate(() => window.dispatchEvent(new Event("online")));
  await page.clock.fastForward(2000);
  await expect(page.locator("#save-status")).toHaveText("Ready");
  expect(writes).toBe(0);
  await page.reload();
  await expect(page.locator("#save-status")).toHaveText("Ready");
});

test("creates a linked thought from its topic and finds disconnected notes through search", async ({
  page,
  request,
}, info) => {
  const name = `topic-${info.project.name}.md`;
  const separate = `separate-${info.project.name}.md`;
  await request.post("/api/notes", { data: { name, body: "# A topic\n\nIdeas" } });
  await request.post("/api/notes", { data: { name: separate, body: "# A disconnected thought" } });
  await page.goto(`/?note=${encodeURIComponent(name)}`);
  await page.getByRole("link", { name: "Explore notes", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Explore notes", exact: true });
  await dialog.getByLabel("Search notes", { exact: true }).fill("A disconnected thought");
  await expect(page.locator("#graph-selection")).toContainText("A disconnected thought");
  await dialog.getByLabel("Search notes", { exact: true }).fill("A topic");
  await expect(page.locator("#graph-selection")).toContainText("A topic");
  await dialog.getByRole("button", { name: "New note", exact: true }).click();
  await expect(page.locator("#body")).toHaveValue(
    new RegExp(`\\[A topic\\]\\(${name.replaceAll(".", "\\.")}\\)`),
  );
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const created = new URL(page.url()).searchParams.get("note");
  await page.getByRole("button", { name: "More", exact: true }).click();
  await expect(page.locator("#body")).toHaveValue(/^# [^\n]+\n\n\[A topic\]/);
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const topic = await (await request.get(`/api/notes/${encodeURIComponent(name)}`)).json();
  const updated = await request.put(`/api/notes/${encodeURIComponent(name)}`, {
    headers: { "if-match": topic.revision },
    data: { body: "# Revised topic\n\nIdeas" },
  });
  expect(updated.ok()).toBe(true);
  await page.reload();
  await expect(page.locator("#body")).toHaveValue(/\[Revised topic\]/);
  expect(new URL(page.url()).searchParams.get("note")).toBe(created);
});

test("new notes have editable timestamp titles with stable independent identities", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "New note", exact: true }).click();
  await expect(page.locator("#body")).toHaveValue(/^# \d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d{3}\n\n$/);
  await expect(page.locator(".title-cue,.body-cue")).toHaveCount(0);
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const first = new URL(page.url()).searchParams.get("note");
  await page.getByRole("button", { name: "New note", exact: true }).click();
  await expect(page.locator("#save-status")).toHaveText("Saved");
  expect(new URL(page.url()).searchParams.get("note")).not.toBe(first);
});

test("keeps a requested graph focus when resizing during a delayed response", async ({
  page,
  request,
}, info) => {
  const initial = `initial-focus-${info.project.name}.md`;
  const selected = `selected-focus-${info.project.name}.md`;
  await page.setViewportSize({ width: 1280, height: 900 });
  await request.post("/api/notes", { data: { name: initial, body: "# Initial focus" } });
  await request.post("/api/notes", { data: { name: selected, body: "# Selected focus" } });
  let releaseNote: () => void = () => {};
  const opening = new Promise<void>((resolve) => {
    releaseNote = resolve;
  });
  await page.route(`**/api/notes/${initial}`, async (route) => {
    await opening;
    await route.continue();
  });
  await page.goto(`/?note=${initial}`);
  await page.getByRole("link", { name: "Explore notes", exact: true }).click();
  await expect(page.locator("#graph-selection")).toContainText("Initial focus");
  releaseNote();
  let release: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  let received: () => void = () => {};
  const requested = new Promise<void>((resolve) => {
    received = resolve;
  });
  await page.route("**/api/graph?*", async (route) => {
    if (new URL(route.request().url()).searchParams.get("name") === selected) {
      received();
      await held;
    }
    await route.continue();
  });
  await page.getByLabel("Search notes", { exact: true }).fill("Selected focus");
  await requested;
  const resized = page.waitForRequest((request) => {
    const url = new URL(request.url());
    return url.pathname === "/api/graph" && url.searchParams.get("limit") === "8";
  });
  await page.locator("#home-graph").evaluate((node) => {
    node.style.width = "160px";
  });
  await resized;
  release();
  await expect(page.locator("#graph-selection")).toContainText("Selected focus");
});
