import { expect, test } from "./editor.js";

function palette(css: string | null): string[] {
  const findings: string[] = [];
  const sheets: CSSStyleSheet[] = [];
  if (css === null)
    sheets.push(
      ...Array.from(document.styleSheets).filter(
        (sheet) => !(sheet.ownerNode instanceof HTMLStyleElement && sheet.ownerNode.nonce),
      ),
    );
  else {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(css);
    sheets.push(sheet);
  }
  const allowed = new Set([
    "black",
    "white",
    "#000",
    "#fff",
    "rgb(0, 0, 0)",
    "rgb(255, 255, 255)",
    "currentcolor",
  ]);
  const colors = [
    "color",
    "background-color",
    "border-top-color",
    "border-bottom-color",
    "border-left-color",
    "border-right-color",
    "outline-color",
    "text-decoration-color",
    "caret-color",
    "fill",
    "stroke",
  ];
  function inspect(rules: CSSRuleList): void {
    for (const rule of Array.from(rules)) {
      if (rule instanceof CSSStyleRule) {
        const style = rule.style;
        for (const property of colors) {
          const value = style.getPropertyValue(property).trim().toLowerCase();
          if (
            value &&
            !allowed.has(value) &&
            !((property === "fill" || property === "stroke") && value === "none") &&
            !(property === "text-decoration-color" && value === "initial")
          )
            findings.push(`${property}: ${value}`);
        }
        for (const property of [
          "background-image",
          "box-shadow",
          "text-shadow",
          "filter",
          "backdrop-filter",
        ]) {
          const value = style.getPropertyValue(property);
          if (value && value !== "none" && value !== "initial") findings.push(property);
        }
        if (style.opacity && Number(style.opacity) !== 1) findings.push("opacity");
        for (const property of ["mix-blend-mode", "background-blend-mode"]) {
          const value = style.getPropertyValue(property);
          if (value && value !== "normal") findings.push(property);
        }
      }
      if (rule instanceof CSSGroupingRule) inspect(rule.cssRules);
    }
  }
  for (const sheet of sheets) inspect(sheet.cssRules);
  return findings;
}

test("enforces the palette and rejects prohibited style fixtures", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByLabel("Note", { exact: true })).toBeEditable();
  expect(await page.evaluate(palette, null)).toEqual([]);
  expect(
    await page.evaluate(
      palette,
      "body { color: black; background: white; border: 1px solid black; }",
    ),
  ).toEqual([]);
  for (const bad of [
    "color: gray",
    "background: linear-gradient(black, white)",
    "opacity: .5",
    "box-shadow: 1px 1px black",
    "text-shadow: 1px 1px black",
    "filter: blur(1px)",
    "backdrop-filter: blur(1px)",
    "mix-blend-mode: multiply",
    "background-color: rgba(0,0,0,.5)",
  ]) {
    expect((await page.evaluate(palette, `.bad { ${bad}; }`)).length, bad).toBeGreaterThan(0);
  }
  const badTargets = await page.locator("button, input, textarea, a").evaluateAll((controls) =>
    controls
      .filter((control) => {
        const rect = control.getBoundingClientRect();
        return rect.width > 0 && rect.height > 0 && (rect.width < 48 || rect.height < 48);
      })
      .map((control) => control.id || control.textContent),
  );
  expect(badTargets).toEqual([]);
  const colors = await page.locator("body *").evaluateAll((elements) =>
    elements
      .filter((element) => {
        const style = getComputedStyle(element);
        return (
          !["rgb(0, 0, 0)", "rgb(255, 255, 255)"].includes(style.color) || style.opacity !== "1"
        );
      })
      .map((element) => element.tagName),
  );
  expect(colors).toEqual([]);
});

test("bounds the writing measure", async ({ page }) => {
  await page.goto("/");
  const textarea = page.getByLabel("Note", { exact: true });
  await expect(textarea).toBeEditable();
  const reading = await textarea.evaluate((input) => {
    const style = getComputedStyle(input);
    return {
      width:
        input.clientWidth -
        Number.parseFloat(style.paddingLeft) -
        Number.parseFloat(style.paddingRight),
      maximum: Number.parseFloat(style.fontSize) * 40,
    };
  });
  expect(reading.width).toBeLessThanOrEqual(reading.maximum + 1);
});

test("captures the editor at desktop and mobile sizes", async ({ page, request }, info) => {
  const response = await request.post("/api/notes", {
    data: {
      name: `Scratchpad-${info.project.name}.md`,
      body: "# Scratchpad\n\nWrite a note.\n\nKeep the Markdown file.\nFind the text when you need it.\n",
    },
  });
  expect(response.ok()).toBeTruthy();
  const note: { name: string } = await response.json();
  await page.goto("/");
  await expect(page.getByLabel("Note", { exact: true })).toBeEditable();
  await page.screenshot({ path: `../target/screenshots/${info.project.name}.png`, fullPage: true });
  const saved = await request.get(`/api/notes/${encodeURIComponent(note.name)}`);
  await request.delete(`/api/notes/${encodeURIComponent(note.name)}`, {
    headers: { "content-type": "application/json", "if-match": saved.headers().etag ?? "" },
  });
});
