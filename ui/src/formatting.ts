import { normalizeHeadings } from "./headings.ts";

export async function formatMarkdown(text: string): Promise<string> {
  if (text.length > 64 * 1024 || !text.trim()) return text;
  const [{ format }, markdown] = await Promise.all([
    import("prettier/standalone"),
    import("prettier/plugins/markdown"),
  ]);
  return format(normalizeHeadings(text), {
    parser: "markdown",
    plugins: [markdown],
    proseWrap: "preserve",
    embeddedLanguageFormatting: "off",
    tabWidth: 2,
    printWidth: 80,
  });
}

export function mappedCursor(before: string, after: string, offset: number): number {
  let prefix = 0;
  while (prefix < before.length && prefix < after.length && before[prefix] === after[prefix])
    prefix++;
  if (offset <= prefix) return offset;
  let suffix = 0;
  while (
    suffix < before.length - prefix &&
    suffix < after.length - prefix &&
    before[before.length - suffix - 1] === after[after.length - suffix - 1]
  )
    suffix++;
  return offset >= before.length - suffix
    ? Math.max(prefix, after.length - (before.length - offset))
    : Math.min(after.length - suffix, offset);
}
