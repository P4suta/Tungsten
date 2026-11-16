import { expect, test } from "@playwright/test";
import { notePage } from "../src/catalog.js";
import { accessibilityViolations } from "../tests/accessibility.js";
import { openNavigation, showPaper } from "../tests/interface.js";

test("searches and follows links in the generated notebook", async ({ page, request }, info) => {
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
  const navigation = await openNavigation(page);
  await navigation.getByLabel("Search all notes", { exact: true }).fill("Ethics");
  const rows = navigation.locator("#sidebar-notes a");
  await expect(rows.first()).toBeVisible();
  expect(await rows.count()).toBeLessThanOrEqual(50);
  const ethics = navigation.getByRole("link", { name: "Ethics", exact: true });
  await expect(ethics).toBeVisible();
  await page.screenshot({
    path: `../target/screenshots/demo-notes-${info.project.name}.png`,
    fullPage: true,
  });
  expect(await accessibilityViolations(page)).toEqual([]);
  await ethics.click();
  await expect(page.locator("#body")).toHaveValue(/^# Ethics\n/);
  const matches = notePage(
    await (await request.get("/api/notes?paged=true&titles=true&q=Ethics")).json(),
  );
  const related = matches.notes.find((note) => note.title.startsWith("Ethics: "));
  if (!related) throw new Error("Missing related Ethics note.");
  await openNavigation(page);
  await page.getByLabel("Search all notes", { exact: true }).fill(related.title);
  await page
    .locator("#sidebar-notes")
    .getByRole("link", { name: related.title, exact: true })
    .click();
  await expect(page.locator("#body")).toHaveValue(/^# Ethics: /);
  expect(new URL(page.url()).searchParams.get("note")).toBe(related.name);
  await showPaper(page);
  expect(failures).toEqual([]);
});
