import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";
import { openNavigation, showPaper } from "./interface.js";

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

test("keeps typing responsive under CPU slowdown", async ({ page, request, browserName }, info) => {
  test.skip(browserName !== "chromium", "CPU emulation requires the Chrome DevTools Protocol");
  for (let index = 0; index < 55; index++)
    await request.post("/api/notes", {
      data: {
        name: `performance-shelf-${info.project.name}-${index}.md`,
        body: `# Performance shelf ${index}\n`,
      },
    });
  await page.goto("/");
  const navigation = await openNavigation(page);
  await expect(navigation.locator("#sidebar-notes a")).toHaveCount(50);
  await showPaper(page);
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

test("bounds retained editor memory when switching many notes", async ({
  page,
  request,
  browserName,
}) => {
  test.skip(browserName !== "chromium", "Heap measurements require the Chrome DevTools Protocol");
  const prefix = `memory-${randomUUID()}`;
  for (let index = 0; index < 24; index++) {
    const response = await request.post("/api/notes", {
      data: {
        name: `${prefix}-${index}.md`,
        body: `# Memory ${prefix} ${index}\n\n${"A paragraph connecting ideas.\n\n".repeat(120)}`,
      },
    });
    expect(response.ok()).toBe(true);
  }
  await page.goto(`/?note=${prefix}-0.md`);
  await expect(page.locator(".cm-content")).toBeEditable();
  const navigation = await openNavigation(page);
  await navigation.locator("#sidebar-search").fill(prefix);
  await expect(navigation.locator("#sidebar-notes a")).toHaveCount(24);
  await showPaper(page);
  const session = await page.context().newCDPSession(page);
  await session.send("Performance.enable");
  const sample = async (): Promise<{ heap: number; nodes: number; listeners: number }> => {
    await session.send("HeapProfiler.collectGarbage");
    const result: { metrics: { name: string; value: number }[] } =
      await session.send("Performance.getMetrics");
    const heap = result.metrics.find((metric) => metric.name === "JSHeapUsedSize")?.value;
    if (heap === undefined || !Number.isFinite(heap) || heap <= 0)
      throw new Error("Missing browser heap measurement");
    const dom: { nodes: number; jsEventListeners: number } =
      await session.send("Memory.getDOMCounters");
    return { heap, nodes: dom.nodes, listeners: dom.jsEventListeners };
  };
  const before = await sample();
  for (let index = 1; index < 24; index++) {
    const navigation = await openNavigation(page);
    await navigation.getByRole("link", { name: `Memory ${prefix} ${index}`, exact: true }).click();
    await expect(page.locator("#body")).toHaveValue(new RegExp(`^# Memory ${prefix} ${index}\\n`));
    await showPaper(page);
  }
  const after = await sample();
  console.log(
    `operation=browser_memory before_bytes=${before.heap} after_bytes=${after.heap} before_nodes=${before.nodes} after_nodes=${after.nodes} before_listeners=${before.listeners} after_listeners=${after.listeners}`,
  );
  expect(
    await page.evaluate(() =>
      performance
        .getEntriesByType("resource")
        .some((entry) => /\/(?:standalone|markdown)[^/]*\.js$/.test(new URL(entry.name).pathname)),
    ),
  ).toBe(false);
  expect(before.heap).toBeLessThan(16 * 1024 * 1024);
  expect(after.heap).toBeLessThan(16 * 1024 * 1024);
  expect(after.heap - before.heap).toBeLessThan(2 * 1024 * 1024);
  expect(after.nodes - before.nodes).toBeLessThan(200);
  expect(after.listeners - before.listeners).toBeLessThan(30);
  await session.detach();
});
