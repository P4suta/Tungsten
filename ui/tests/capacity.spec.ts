import { expect, test } from "@playwright/test";

test("edits a near-limit note without creating thousands of preview elements", async ({
  page,
  request,
}) => {
  const body = `# Capacity\n\n${"## Heading 日本語\n\n".repeat(35_000)}`;
  const created = await request.post("/api/notes", { data: { name: "capacity.md", body } });
  expect(created.ok()).toBe(true);
  const saved: { name: string; revision: string } = await created.json();
  await page.goto(`/?note=${saved.name}`);
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await expect(editor).toBeEditable();
  expect(await editor.locator("*").count()).toBeLessThan(300);
  await editor.press("ControlOrMeta+Home");
  await editor.press("End");
  await editor.pressSequentially(" edited");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const updated = await request.get(`/api/notes/${saved.name}`);
  const note: { body: string; revision: string } = await updated.json();
  expect(note.body).toBe(body.replace("# Capacity", "# Capacity edited"));
  await request.delete(`/api/notes/${saved.name}`, {
    headers: { "If-Match": note.revision, "content-type": "application/json" },
  });
});
