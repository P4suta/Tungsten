import { readFile } from "node:fs/promises";
import { accessibilityViolations } from "./accessibility.js";
import { expect, test } from "./editor.js";
import { attachToNote, newNote, openTrash } from "./interface.js";

test.beforeEach(async ({ page, request }) => {
  const response = await request.get("/api/notes");
  const notes: { name: string }[] = await response.json();
  for (const note of notes) {
    const current = await request.get(`/api/notes/${encodeURIComponent(note.name)}`);
    const saved: { revision: string } = await current.json();
    await request.delete(`/api/notes/${encodeURIComponent(note.name)}`, {
      headers: { "If-Match": saved.revision, "content-type": "application/json" },
    });
  }
  await page.goto("/");
  await expect(page.getByLabel("Note", { exact: true })).toBeEditable();
});

test("keeps recordings and PDF pages in place during live editing", async ({ page, request }) => {
  const audioBytes = recording();
  const pdfBytes = documentPdf();
  const audioFile: { id: string } = await (
    await request.post("/api/attachments", {
      data: { name: "live.wav", data: audioBytes.toString("base64") },
    })
  ).json();
  const pdfFile: { id: string } = await (
    await request.post("/api/attachments", {
      data: { name: "live.pdf", data: pdfBytes.toString("base64") },
    })
  ).json();
  await page
    .getByLabel("Note", { exact: true })
    .fill(
      `# Live media\n\n[Recording](.attachments/${audioFile.id})\n\n[Document](.attachments/${pdfFile.id})\n\nTail`,
    );
  await expect(page.locator("#save-status")).toHaveText("Saved");
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.press("ControlOrMeta+End");
  const audio = page.locator("#live-editor audio");
  await expect
    .poll(() => audio.evaluate((node: HTMLAudioElement) => node.readyState))
    .toBeGreaterThanOrEqual(1);
  await audio.evaluate(async (node: HTMLAudioElement) => {
    node.muted = true;
    node.dataset.continuity = "playing";
    await node.play();
  });
  await expect
    .poll(() => audio.evaluate((node: HTMLAudioElement) => node.currentTime))
    .toBeGreaterThan(0.05);
  const pdf = page.locator("#live-editor .file-preview").filter({ hasText: "live.pdf" });
  await expect(pdf.getByText("Page 1 of 2", { exact: true })).toBeVisible();
  await pdf.getByRole("button", { name: "Next page", exact: true }).click();
  await expect(pdf.getByText("Page 2 of 2", { exact: true })).toBeVisible();
  await editor.press("ControlOrMeta+Home");
  await editor.press("End");
  await editor.pressSequentially(" edited");
  await expect(audio).toHaveAttribute("data-continuity", "playing");
  await expect(pdf.getByText("Page 2 of 2", { exact: true })).toBeVisible();
  expect(await (await request.get(`/.attachments/${audioFile.id}`)).body()).toEqual(audioBytes);
  expect(await (await request.get(`/.attachments/${pdfFile.id}`)).body()).toEqual(pdfBytes);
});

function recording(): Buffer {
  const result = Buffer.alloc(44 + 16000);
  result.write("RIFF", 0);
  result.writeUInt32LE(result.length - 8, 4);
  result.write("WAVEfmt ", 8);
  result.writeUInt32LE(16, 16);
  result.writeUInt16LE(1, 20);
  result.writeUInt16LE(1, 22);
  result.writeUInt32LE(8000, 24);
  result.writeUInt32LE(16000, 28);
  result.writeUInt16LE(2, 32);
  result.writeUInt16LE(16, 34);
  result.write("data", 36);
  result.writeUInt32LE(16000, 40);
  return result;
}

function documentPdf(): Buffer {
  const first = "BT /F1 18 Tf 20 150 Td (First page) Tj ET";
  const second = "BT /F1 18 Tf 20 150 Td (Second page) Tj ET";
  const objects = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 240 200] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R /Annots [8 0 R] >>",
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    `<< /Length ${first.length} >>\nstream\n${first}\nendstream`,
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 240 200] /Resources << /Font << /F1 4 0 R >> >> /Contents 7 0 R >>",
    `<< /Length ${second.length} >>\nstream\n${second}\nendstream`,
    "<< /Type /Annot /Subtype /Link /Rect [20 20 100 40] /A << /S /URI /URI (https://example.test/destination) >> >>",
  ];
  let text = "%PDF-1.4\n";
  const offsets: number[] = [0];
  for (const [index, object] of objects.entries()) {
    offsets.push(text.length);
    text += `${index + 1} 0 obj\n${object}\nendobj\n`;
  }
  const start = text.length;
  text += `xref\n0 ${offsets.length}\n0000000000 65535 f \n${offsets
    .slice(1)
    .map((offset) => `${String(offset).padStart(10, "0")} 00000 n \n`)
    .join("")}trailer\n<< /Size ${offsets.length} /Root 1 0 R >>\nstartxref\n${start}\n%%EOF`;
  return Buffer.from(text);
}

test("displays images, plays recordings, and renders PDF pages with working links", async ({
  page,
  context,
}, info) => {
  await page
    .getByLabel("Note", { exact: true })
    .fill("Media\n\n[Web link](https://example.test/destination)\n\n");
  await attachToNote(page, [
    { name: "recording.wav", mimeType: "audio/wav", buffer: recording() },
    { name: "report.pdf", mimeType: "application/pdf", buffer: documentPdf() },
    {
      name: "clip.mp4",
      mimeType: "video/mp4",
      buffer: await readFile(new URL("./fixtures/clip.mp4", import.meta.url)),
    },
    {
      name: "pixel.png",
      mimeType: "image/png",
      buffer: Buffer.from(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
        "base64",
      ),
    },
  ]);
  await expect(page.getByLabel("Note", { exact: true })).toHaveMarkdown(/pixel\.png/);
  await page.getByLabel("Note", { exact: true }).press("ControlOrMeta+Home");
  await expect
    .poll(() =>
      page
        .locator("#live-editor img")
        .evaluate((node) => node instanceof HTMLImageElement && node.naturalWidth),
    )
    .toBe(1);
  const audio = page.locator("#live-editor audio");
  await expect
    .poll(() => audio.evaluate((node) => node instanceof HTMLAudioElement && node.readyState))
    .toBeGreaterThanOrEqual(1);
  await audio.evaluate(async (node) => {
    if (!(node instanceof HTMLAudioElement)) throw new Error("Missing audio");
    node.muted = true;
    await node.play();
  });
  await expect
    .poll(() => audio.evaluate((node) => node instanceof HTMLAudioElement && node.currentTime))
    .toBeGreaterThan(0.05);
  await audio.evaluate((node) => {
    if (node instanceof HTMLAudioElement) node.pause();
  });
  await expect
    .poll(() =>
      page
        .locator("#live-editor video")
        .evaluate((node) => node instanceof HTMLVideoElement && node.videoWidth),
    )
    .toBe(32);
  const pdf = page.locator(".file-preview").filter({ hasText: "report.pdf" });
  await expect(pdf.getByText("Page 1 of 2", { exact: true })).toBeVisible();
  await expect
    .poll(() =>
      pdf.locator("canvas").evaluate((node) => {
        if (!(node instanceof HTMLCanvasElement)) return false;
        const pixels = node.getContext("2d")?.getImageData(0, 0, node.width, node.height).data;
        return pixels?.some((value, index) => index % 4 !== 3 && value < 100) ?? false;
      }),
    )
    .toBe(true);
  await pdf.getByText("Page text", { exact: true }).click();
  await expect(pdf.locator(".pdf-text")).toContainText("First page");
  await context.route("https://example.test/destination", (route) =>
    route.fulfill({ contentType: "text/html", body: "Destination" }),
  );
  const popup = page.waitForEvent("popup");
  await page.getByRole("link", { name: "Web link", exact: true }).click();
  const destination = await popup;
  await expect(destination).toHaveURL("https://example.test/destination");
  await destination.close();
  await expect(
    pdf.getByRole("link", { name: "https://example.test/destination", exact: true }).locator("a"),
  ).toHaveAttribute("href", "https://example.test/destination");
  await pdf.getByRole("button", { name: "Next page", exact: true }).click();
  await expect(pdf.getByText("Page 2 of 2", { exact: true })).toBeVisible();
  await expect(pdf.locator(".pdf-text")).toContainText("Second page");
  expect(await accessibilityViolations(page)).toEqual([]);
  await page.screenshot({
    path: `../target/screenshots/media-${info.project.name}.png`,
    fullPage: true,
  });
});

test("pastes and drops original files into the live note", async ({ page }) => {
  const note = page.getByLabel("Note", { exact: true });
  await note.fill("# Attached\n\n");
  await expect(page.locator("#body")).toHaveValue("# Attached\n\n");
  for (const type of ["paste", "drop"]) {
    const name = `${type}.txt`;
    const count = await note.evaluate(
      (input, { type, name }) => {
        const transfer = new DataTransfer();
        transfer.items.add(new File(["original bytes"], name, { type: "text/plain" }));
        const event =
          type === "paste"
            ? new ClipboardEvent("paste", { bubbles: true, cancelable: true })
            : new DragEvent("drop", { bubbles: true, cancelable: true });
        Object.defineProperty(event, type === "paste" ? "clipboardData" : "dataTransfer", {
          value: transfer,
        });
        input.dispatchEvent(event);
        return transfer.files.length;
      },
      { type, name },
    );
    expect(count).toBe(1);
    await expect
      .poll(() => page.locator("#body").inputValue())
      .toContain(`[${name}](.attachments/`);
  }
  await expect(page.locator("#save-status")).toHaveText("Saved");
});

test("ignores retired appearance preferences and supports browser enlargement", async ({
  page,
}) => {
  await page.evaluate(() =>
    localStorage.setItem("tungsten.preferences", '{"textSize":200,"pageWidth":"full"}'),
  );
  await page.reload();
  await expect(page.locator("#editor-pane")).toHaveCSS("max-width", "774px");
  await page.setViewportSize({ width: 320, height: 900 });
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "225%";
  });
  await expect(page.locator("#sidebar")).not.toHaveAttribute("popover");
  const trash = await openTrash(page);
  expect(await trash.evaluate((node) => node.scrollWidth <= node.clientWidth + 1)).toBe(true);
  expect(await accessibilityViolations(page)).toEqual([]);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("button", { name: "Navigation", exact: true })).toBeFocused();
});

test("creates internal note identities when randomUUID is unavailable", async ({ page }) => {
  await page.evaluate(() => Object.defineProperty(crypto, "randomUUID", { value: undefined }));
  await newNote(page);
  await page.getByLabel("Note", { exact: true }).fill("# First idea\n");
  await expect(page.locator("#save-status")).toHaveText("Saved");
  expect(new URL(page.url()).searchParams.get("note")).toMatch(/^[a-f0-9]{32}\.md$/);
  await expect(page.locator("#name, #zettelkasten, #zettel-folder, #linked-note-form")).toHaveCount(
    0,
  );
});

test("keeps a PDF page selected while writing below its preview", async ({ page, request }) => {
  const file: { id: string } = await (
    await request.post("/api/attachments", {
      data: { name: "report.pdf", data: documentPdf().toString("base64") },
    })
  ).json();
  const recordingFile: { id: string } = await (
    await request.post("/api/attachments", {
      data: { name: "recording.wav", data: recording().toString("base64") },
    })
  ).json();
  const source = `[Report](.attachments/${file.id}) [Recording](.attachments/${recordingFile.id})\n\nNotes`;
  const editor = page.getByLabel("Note", { exact: true });
  await editor.fill(source);
  await editor.press("ControlOrMeta+End");
  await expect(page.getByRole("status").filter({ hasText: /^Saved$/ })).toBeVisible();
  await expect(page.getByText("Page 1 of 2", { exact: true })).toBeVisible();
  await expect(page.locator(".file-preview > p:first-child")).toHaveText([
    "report.pdf",
    "recording.wav",
  ]);
  await page.getByRole("button", { name: "Next page", exact: true }).click();
  await expect(page.getByText("Page 2 of 2", { exact: true })).toBeVisible();
  const audio = page.locator("#live-editor audio");
  await audio.evaluate(async (node) => {
    if (!(node instanceof HTMLAudioElement)) throw new Error("Missing audio");
    node.muted = true;
    node.loop = true;
    await node.play();
  });
  await editor.press("ControlOrMeta+End");
  await editor.press("Enter");
  await editor.press("Enter");
  await editor.pressSequentially("More notes");
  await expect(page.locator("#live-editor")).toContainText("More notes");
  await expect(page.getByText("Page 2 of 2", { exact: true })).toBeVisible();
  await expect
    .poll(() =>
      audio.evaluate(
        (node) => node instanceof HTMLAudioElement && !node.paused && node.currentTime > 0,
      ),
    )
    .toBe(true);
});
