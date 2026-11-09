import assert from "node:assert/strict";
import { after, test } from "node:test";
import { Window } from "happy-dom";
import { codeHighlights } from "../src/syntax.ts";

const window = new Window();
Object.assign(globalThis, {
  document: window.document,
  Node: window.Node,
  NodeFilter: window.NodeFilter,
  HTMLElement: window.HTMLElement,
});
after(() => window.happyDOM.abort());

test("highlights explicit language names and aliases without changing literal code", async () => {
  for (const [language, code, token] of [
    ["js", "const name = 'value';", "const"],
    ["python extra", "import package\nprint('日本語')", "import"],
    ["rust", "let answer: i32 = 42;", "let"],
    ["sql", "SELECT * FROM notes;", "SELECT"],
    ["bash", 'echo "hello"', "echo"],
    ["html", "<script>alert('text')</script>", "script"],
    ["haskell", 'module Main where\nmain = putStrLn "hello"', "module"],
    ["ruby", "def hello\n  puts 'hello'\nend", "def"],
  ] as const) {
    const source = `# Title\n\n\`\`\`${language}\n${code}\n\`\`\``;
    const highlights = await codeHighlights(source);
    assert.ok(
      highlights.some((range) => source.slice(range.from, range.to).includes(token ?? "")),
      language,
    );
    for (const range of highlights) {
      assert.ok(range.from >= source.indexOf("\n", source.indexOf("```")) + 1);
      assert.ok(range.to <= source.lastIndexOf("```"));
      assert.match(range.className, /(?:^| )hljs-[a-z0-9_-]+/);
      assert.match(range.className, /^[a-z0-9_ -]+$/);
    }
  }
  assert.deepEqual(await codeHighlights("```hogepiyo lang\nimport package\n```"), []);
  assert.deepEqual(await codeHighlights("```\nconst name = 1;\n```"), []);
  assert.deepEqual(await codeHighlights("`const name = 1;`"), []);
});
