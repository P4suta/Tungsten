import { expect, test } from "@playwright/test";

test("loads only the editor before secondary tools and bounds cold JavaScript", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByRole("textbox", { name: "Note", exact: true })).toBeEditable();
  const scripts = await page.evaluate(() =>
    performance
      .getEntriesByType("resource")
      .filter(
        (entry): entry is PerformanceResourceTiming => entry instanceof PerformanceResourceTiming,
      )
      .filter((entry) => entry.name.endsWith(".js") || entry.name.endsWith(".mjs"))
      .map((entry) => ({
        name: new URL(entry.name).pathname,
        encoded: entry.encodedBodySize,
        decoded: entry.decodedBodySize,
      })),
  );
  expect(
    scripts.some((script) =>
      /pdf|syntax|markdown|standalone|d3-force|differences/.test(script.name),
    ),
  ).toBe(false);
  expect(scripts.reduce((sum, script) => sum + script.encoded, 0)).toBeGreaterThan(0);
  expect(scripts.reduce((sum, script) => sum + script.encoded, 0)).toBeLessThan(180 * 1024);
  expect(scripts.reduce((sum, script) => sum + script.decoded, 0)).toBeLessThan(420 * 1024);
});

test("keeps typing responsive under CPU slowdown", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "CPU emulation requires the Chrome DevTools Protocol");
  await page.goto("/");
  const editor = page.getByRole("textbox", { name: "Note", exact: true });
  await editor.fill(`# Input latency\n\n${"A line connecting thoughts.\n\n".repeat(300)}`);
  await editor.press("ControlOrMeta+End");
  const session = await page.context().newCDPSession(page);
  await session.send("Emulation.setCPUThrottlingRate", { rate: 6 });
  const timings: number[] = [];
  for (let index = 0; index < 12; index++) {
    const start = performance.now();
    await editor.press("x");
    await page.evaluate(
      () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve())),
    );
    timings.push(performance.now() - start);
  }
  timings.sort((a, b) => a - b);
  expect(timings[10]).toBeLessThan(200);
  await session.send("Emulation.setCPUThrottlingRate", { rate: 1 });
  await session.detach();
  await expect(page.locator("#body")).toHaveValue(/x{12}$/);
});
