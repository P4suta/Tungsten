import { expect, test } from "@playwright/test";
import { newNote } from "./interface.js";

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

test("new notes have editable timestamp titles with stable independent identities", async ({
  page,
}) => {
  await page.goto("/");
  await newNote(page);
  await expect(page.locator("#body")).toHaveValue(/^# \d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d{3}\n\n$/);
  await expect(page.locator(".title-cue,.body-cue")).toHaveCount(0);
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const first = new URL(page.url()).searchParams.get("note");
  await newNote(page);
  await expect(page.locator("#save-status")).toHaveText("Saved");
  expect(new URL(page.url()).searchParams.get("note")).not.toBe(first);
});
