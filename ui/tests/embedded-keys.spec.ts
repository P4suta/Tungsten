import { expect, test } from "@playwright/test";
import { accessibilityViolations } from "./accessibility.js";
import { missingIcons } from "./icons-check.js";
import { newEmptyNote } from "./interface.js";

test("keeps Enter available to a link embedded in the live editor", async ({
  page,
  request,
}, info) => {
  const target = `embedded-${info.project.name}.md`;
  await request.post("/api/notes", { data: { name: target, body: "# Linked destination" } });
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.press("ControlOrMeta+a");
  await page.keyboard.insertText(`# Link\n\n[Destination](${target})\n\nAfter`);
  const link = page.locator(".live-block").getByRole("link", { name: "Destination", exact: true });
  await expect(link).toBeVisible();
  await link.focus();
  await expect(link).toBeFocused();
  expect(await accessibilityViolations(page)).toEqual([]);
  expect(await page.evaluate(missingIcons, null)).toEqual([]);
  await link.press("Enter");
  await expect(page.locator("#body")).toHaveValue("# Linked destination");
  await expect(editor).toBeFocused();
  await page.keyboard.press("ControlOrMeta+End");
  await page.keyboard.insertText(" continued");
  await expect(page.locator("#body")).toHaveValue("# Linked destination continued");
});

test("keeps generated image links keyboard accessible", async ({ page, request }) => {
  const file: { url: string } = await (
    await request.post("/api/attachments", {
      data: {
        name: "keyboard.png",
        data: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jVfoAAAAASUVORK5CYII=",
      },
    })
  ).json();
  await page.goto("/");
  await newEmptyNote(page);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.press("ControlOrMeta+a");
  await page.keyboard.insertText(`# Image\n\n![keyboard.png](${file.url})\n\nAfter`);
  const link = page.getByRole("link", { name: "Open keyboard.png", exact: true });
  await expect(link).toBeVisible();
  await link.focus();
  await expect(link).toBeFocused();
  const opening = page.waitForEvent("popup");
  await link.press("Enter");
  const popup = await opening;
  await expect(popup).toHaveURL(new URL(file.url, page.url()).href);
  await popup.close();
});
