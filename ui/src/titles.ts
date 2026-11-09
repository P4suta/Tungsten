export function noteTitle(text: string): string {
  const heading = /^#\s+(.+)$/m.exec(text)?.[1];
  const first = text.split("\n").find((line) => line.trim()) ?? "";
  return (
    (heading ?? first)
      .replace(/^#+\s*/, "")
      .trim()
      .slice(0, 120) || "Untitled note"
  );
}

export function defaultTitle(date: Date): string {
  const part = (value: number, width = 2): string => String(value).padStart(width, "0");
  return `${date.getFullYear()}-${part(date.getMonth() + 1)}-${part(date.getDate())} ${part(date.getHours())}:${part(date.getMinutes())}:${part(date.getSeconds())}.${part(date.getMilliseconds(), 3)}`;
}
