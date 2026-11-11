import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import type { AxeResults } from "axe-core";

export async function accessibilityViolations(page: Page): Promise<AxeResults["violations"]> {
  if (page.frames().length !== 1)
    throw new Error("The accessibility gate requires a page without child frames.");
  const result = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
    .setLegacyMode()
    .analyze();
  return result.violations;
}
