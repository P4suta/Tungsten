import { expect, test } from "@playwright/test";
import { notePage } from "../src/catalog.js";
import { accessibilityViolations } from "../tests/accessibility.js";

test("explores and searches the generated notebook", async ({ page, request }, info) => {
  const response = await request.get("/api/notes?paged=true&titles=true&q=Philosophy");
  expect(response.ok()).toBe(true);
  const collection = notePage(await response.json());
  expect(collection.total).toBe(Number(process.env.TUNGSTEN_DEMO_COUNT ?? 10_000));
  const notes = collection.notes;
  expect(notes.length).toBeLessThanOrEqual(50);
  const entry: unknown = notes.find(
    (note: unknown) =>
      typeof note === "object" && note !== null && "title" in note && note.title === "Philosophy",
  );
  if (
    typeof entry !== "object" ||
    entry === null ||
    !("name" in entry) ||
    typeof entry.name !== "string"
  )
    throw new Error("Missing Philosophy note");
  const failures: string[] = [];
  page.on("pageerror", (error) => failures.push(error.message));
  await page.goto(`/?note=${encodeURIComponent(entry.name)}`);
  await expect(page.locator("#body")).toHaveValue(/^# Philosophy\n/);
  await page.getByRole("link", { name: "Explore notes", exact: true }).click();
  const graph = page.getByRole("region", { name: "Note graph", exact: true });
  await expect(graph.locator(".graph-node").first()).toBeVisible();
  expect(await graph.locator(".graph-node").count()).toBeLessThanOrEqual(120);
  expect(await graph.locator(".graph-node").count()).toBeGreaterThan(1);
  await expect(page.locator("#graph-status")).toContainText("notes");
  await expect(page.locator("#graph-selection")).toContainText("Philosophy");
  const ethics = graph.getByRole("button", { name: "Explore Ethics", exact: true });
  if ((await ethics.count()) === 0)
    await page.getByLabel("Search notes", { exact: true }).fill("Ethics");
  await ethics.click();
  await expect(page.locator("#graph-selection")).toContainText("Ethics");
  await page.screenshot({
    path: `../target/screenshots/demo-graph-${info.project.name}.png`,
    fullPage: true,
  });
  expect(await accessibilityViolations(page)).toEqual([]);
  await page.getByRole("link", { name: "Open note", exact: true }).click();
  await expect(page.locator("#body")).toHaveValue(/^# Ethics\n/);
  await page.getByRole("link", { name: "Explore notes", exact: true }).click();
  await page.getByLabel("Search notes", { exact: true }).fill("Ethics: A counterexample");
  await page
    .locator("#notes")
    .getByRole("button", { name: "Ethics: A counterexample", exact: true })
    .click();
  await expect(page.locator("#body")).toHaveValue(/^# Ethics: A counterexample\n/);
  expect(failures).toEqual([]);
});
