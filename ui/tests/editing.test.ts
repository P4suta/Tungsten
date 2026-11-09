import assert from "node:assert/strict";
import { after, test } from "node:test";
import { Window } from "happy-dom";
import {
  completeInput,
  continueLine,
  indentLines,
  initInput,
  removePair,
  type TextSelection,
  tabTarget,
  wrapSelection,
} from "../src/input.ts";
import { needsWidget, textLayout } from "../src/live-ranges.ts";
import { registerSurface } from "../src/surface.ts";

const window = new Window();
Object.assign(globalThis, {
  document: window.document,
  Event: window.Event,
  InputEvent: window.InputEvent,
  KeyboardEvent: window.KeyboardEvent,
});
after(() => window.happyDOM.abort());

function cursor(text: string, start = text.length, end = start): TextSelection {
  return { text, start, end };
}

test("completion distinguishes word boundaries, unmatched closers, and list markers", () => {
  for (const invalid of [cursor("", -1), cursor("a", 1, 0), cursor("a", 0, 2)])
    assert.equal(completeInput(invalid, "["), null);
  assert.equal(completeInput(cursor(")]", 0), ")]"), null);
  for (const marker of ["*", "**"]) {
    assert.deepEqual(
      wrapSelection(cursor("word", 0, 4), marker),
      cursor(`${marker}word${marker}`, marker.length, marker.length + 4),
    );
    assert.deepEqual(
      wrapSelection(cursor(`${marker}word`, marker.length, marker.length + 4), marker),
      cursor(`${marker}${marker}word${marker}`, marker.length * 2, marker.length * 2 + 4),
    );
    assert.deepEqual(
      wrapSelection(cursor(`word${marker}`, 0, 4), marker),
      cursor(`${marker}word${marker}${marker}`, marker.length, marker.length + 4),
    );
  }
  assert.deepEqual(completeInput(cursor("[word]() tail", 5), "]"), cursor("[word]() tail", 7));
  assert.deepEqual(completeInput(cursor("[word]x", 5), "]"), cursor("[word]x", 6));
  assert.equal(completeInput(cursor("[word]() tail", 5), "x"), null);
  assert.equal(completeInput(cursor("word)", 0, 4), ")"), null);
  assert.equal(completeInput(cursor("aa", 1), "a"), null);
  assert.deepEqual(completeInput(cursor("* x*", 3), "*", true), cursor("* x*", 4));
  assert.deepEqual(completeInput(cursor(" ** rest", 2), "*", true), cursor(" **** rest", 3));
  for (const marker of ["*", "_", "`", '"', "'"])
    for (const word of ["a", "7", "字"])
      assert.equal(completeInput(cursor(`prefix${word}`), marker), null);
  for (const next of [" ", ")", "]", "}", ".", ",", ";", ":", "!", "?"])
    assert.deepEqual(completeInput(cursor(next, 0), "("), cursor(`()${next}`, 1));
  assert.equal(completeInput(cursor("x space", 0), "("), null);
  assert.equal(completeInput(cursor("unclosed]", 0), "a"), null);
  assert.deepEqual(completeInput(cursor("**tail", 1), " "), cursor("* tail", 2));
  assert.deepEqual(completeInput(cursor("\n  **tail", 4), " "), cursor("\n  * tail", 5));
  assert.equal(completeInput(cursor("x**tail", 2), " "), null);
  assert.equal(completeInput(cursor("*x*tail", 2), " "), null);
  assert.equal(completeInput(cursor("* xx", 3), " "), null);
  assert.deepEqual(completeInput(cursor("a\n**tail", 3), " "), cursor("a\n* tail", 4));
  assert.deepEqual(completeInput(cursor("a "), "*"), cursor("a **", 3));
  assert.equal(completeInput(cursor("𠮷"), "_"), null);
  assert.equal(completeInput(cursor("**tail", 1), "x"), null);
  assert.equal(removePair(cursor("()", 1, 2)), null);
  assert.equal(removePair(cursor("()", 0)), null);
  assert.deepEqual(removePair(cursor("[] tail", 1)), cursor(" tail", 0));
  assert.equal(removePair(cursor("x]()", 1)), null);
});

test("continuation distinguishes complete fences, list content, and indentation", () => {
  for (const [source, expected] of [
    ["- ", ""],
    ["  -   ", ""],
    ["12. item", "12. item\n13. "],
    ["009) item", "009) item\n10) "],
    [">item", ">item\n>"],
    ["  > item", "  > item\n  > "],
    [">> one", ">> one\n>> "],
    ["- [a] text", "- [a] text\n- "],
    ["- [ ] text", "- [ ] text\n- [ ] "],
    ["- [X] text", "- [X] text\n- [ ] "],
  ]) {
    assert.ok(source !== undefined && expected !== undefined);
    assert.deepEqual(continueLine(cursor(source)), cursor(expected));
  }
  for (const marker of ["```", "~~~", "````", "~~~~"]) {
    const source = `  ${marker}lang${marker}`;
    assert.deepEqual(
      continueLine(cursor(source, marker.length + 6)),
      cursor(`  ${marker}lang\n  \n  ${marker}`, marker.length + 9),
    );
  }
  for (const text of ["``", "~~~~".slice(0, 2), "x```", "```x`", "~~~x~"])
    assert.equal(continueLine(cursor(text + text, text.length)), null);
  assert.equal(continueLine(cursor("text - one")), null);
  assert.equal(continueLine(cursor("- one", 2, 5)), null);
  assert.deepEqual(continueLine(cursor("a\n- rest", 4)), cursor("a\nrest", 2));
  for (const [text, start, end, expected, selectedFrom, selectedTo] of [
    ["one\ntwo", 0, 4, "    one\ntwo", 4, 8],
    ["one\ntwo", 4, 4, "one\n    two", 8, 8],
    ["one\ntwo", 1, 5, "    one\n    two", 5, 13],
    ["\nfirst", 0, 0, "    \nfirst", 4, 4],
    ["\nfirst", 1, 1, "\n    first", 5, 5],
  ] as const)
    assert.deepEqual(
      indentLines(cursor(text, start, end), 4, false),
      cursor(expected, selectedFrom, selectedTo),
    );
  for (const text of ["plain", "", "  ", "    ", "  \tx"])
    assert.equal(indentLines(cursor(text, 0), 4, true).text, text.replace(/^ {0,4}/, ""));
});

test("Tab respects syntax and selection boundaries and handles nested expressions", () => {
  const text = "before [label](url) after";
  assert.equal(tabTarget(cursor(text, 0)), null);
  assert.equal(tabTarget(cursor(text)), null);
  assert.equal(tabTarget(cursor(text, 15, text.length), true), null);
  assert.deepEqual(tabTarget(cursor("[label](url)", 0)), cursor("[label](url)", 8));
  assert.equal(tabTarget(cursor("[label](url)")), null);
  assert.equal(tabTarget(cursor("[label](url)", 8, 12)), null);
  assert.equal(tabTarget(cursor("[label](url)", 1), true), null);
  assert.deepEqual(tabTarget(cursor("[label](url)", 6)), cursor("[label](url)", 8));
  assert.equal(tabTarget(cursor("* x*", 0)), null);
  assert.equal(tabTarget(cursor("* x*", 1)), null);
  assert.equal(tabTarget(cursor("*a* gap *b*", 6)), null);
  assert.equal(tabTarget(cursor("()", 1, 2)), null);
  assert.equal(tabTarget(cursor(")(", 0)), null);
  assert.equal(tabTarget(cursor("x*", 0)), null);
  assert.deepEqual(tabTarget(cursor(" **", 2)), cursor(" **", 3));
  assert.deepEqual(tabTarget(cursor(" ****", 3)), cursor(" ****", 5));
  assert.deepEqual(tabTarget(cursor("**word**", 1)), cursor("**word**", 2));
  assert.deepEqual(tabTarget(cursor("**word**", 2)), cursor("**word**"));
  assert.equal(tabTarget(cursor("**word**", 2, 7)), null);
  assert.deepEqual(tabTarget(cursor("**a *b* c**", 5)), cursor("**a *b* c**", 7));
});

test("formats each inline expression and reveals only intersecting syntax", () => {
  const text = "**bold** and *italic* and `code` and ~~gone~~";
  const outside = textLayout(text, { from: text.length, to: text.length });
  assert.deepEqual(
    outside.marks.map((mark) => [text.slice(mark.from, mark.to), mark.tag]),
    [
      ["bold", "strong"],
      ["italic", "em"],
      ["code", "code"],
      ["gone", "s"],
    ],
  );
  assert.deepEqual(
    outside.hidden.map((range) => text.slice(range.from, range.to)),
    ["**", "**", "*", "*", "`", "`"],
  );
  for (let caret = 0; caret <= 8; caret++) {
    const layout = textLayout(text, { from: caret, to: caret });
    assert.deepEqual(layout.marks, outside.marks);
    assert.ok(layout.hidden.every((range) => range.from >= 8));
  }
  assert.deepEqual(textLayout(text, { from: 2, to: text.length }).hidden, []);
  assert.deepEqual(textLayout("plain", { from: 0, to: 0 }).marks, []);
  assert.deepEqual(textLayout("plain", { from: 0, to: 0 }), {
    marks: [],
    hidden: [],
    headings: [],
    prefixes: [],
    lines: [],
  });
  assert.deepEqual(textLayout("x **bold**", { from: 0, to: 0 }).hidden, [
    { from: 2, to: 4 },
    { from: 8, to: 10 },
  ]);
  assert.deepEqual(textLayout("#\n", { from: 0, to: 0 }).headings, [{ from: 0, level: 1 }]);
});

test("uses Markdown boundaries for nesting, escapes, code, and Unicode", () => {
  const samples = [
    "前😀 **太字** *斜体*",
    "***nested***",
    "**outer *inner* end**",
    "\\*literal\\*",
    "`**literal**`",
    "```\n**literal**\n```",
    "**incomplete",
  ];
  for (const text of samples) {
    for (let from = 0; from <= text.length; from++) {
      for (const to of [from, text.length]) {
        const plan = textLayout(text, { from, to });
        for (const range of [...plan.marks, ...plan.hidden]) {
          assert.ok(range.from >= 0 && range.from < range.to && range.to <= text.length);
          assert.ok(!/[\uDC00-\uDFFF]/.test(text[range.from] ?? ""));
          assert.ok(!/[\uD800-\uDBFF]/.test(text[range.to - 1] ?? ""));
        }
      }
    }
  }
  for (const text of ["\\*literal\\*", "```\n**literal**\n```", "**incomplete"])
    assert.deepEqual(textLayout(text, { from: 0, to: 0 }).marks, []);
  assert.deepEqual(
    textLayout("***nested***", { from: 12, to: 12 }).marks.map((mark) => mark.tag),
    ["em", "strong"],
  );
  for (let level = 1; level <= 6; level++) {
    const text = `text\n  ${"#".repeat(level)} Title`;
    assert.deepEqual(textLayout(text, { from: 0, to: 0 }).headings, [{ from: 5, level }]);
  }
});

test("reserves HTML widgets for interactive or non-text content", () => {
  for (const tag of ["a", "img", "input", "table", "hr"])
    assert.equal(needsWidget(`<${tag} class="example">`), true);
  for (const html of [
    "<p>plain</p>",
    "<h1>Title</h1>",
    "<ul><li>one</li></ul>",
    "<pre><code>code</code></pre>",
    "<article>text</article>",
    "&lt;img&gt;",
  ])
    assert.equal(needsWidget(html), false);
});

test("navigates link labels and destinations in both directions without changing text", () => {
  for (const text of ["[Label]()", "[Label](https://example.com/a_(b))", "![image](file.jpg)"]) {
    const label = text.indexOf("[") + 1;
    const url = text.indexOf("](") + 2;
    assert.deepEqual(tabTarget(cursor(text, label)), cursor(text, url));
    assert.deepEqual(tabTarget(cursor(text, url)), cursor(text));
    assert.deepEqual(tabTarget(cursor(text, text.length), true), cursor(text, label));
    assert.deepEqual(tabTarget(cursor(text, url), true), cursor(text, label));
  }
  for (const marker of ["*", "**", "_", "__", "`", "``"]) {
    const text = `${marker}word${marker}`;
    assert.deepEqual(tabTarget(cursor(text, marker.length + 2)), cursor(text));
    assert.deepEqual(tabTarget(cursor(text, marker.length + 2), true), cursor(text, marker.length));
  }
  assert.equal(tabTarget(cursor("plain")), null);
  assert.equal(tabTarget(cursor("[reference][label]", 2)), null);
  for (const text of ["()", "[]", "{}", '""', "''", "**", "__", "``"])
    assert.deepEqual(tabTarget(cursor(text, 1)), cursor(text, 2));
  assert.equal(tabTarget(cursor(")", 0)), null);
  assert.equal(tabTarget(cursor("()", 1), true), null);
});

test("completion preserves every surrounding source and keeps selections within bounds", () => {
  const pieces = ["", "a", "日本語", "😀", "\n", "- ", "\\", "**"];
  for (const left of pieces)
    for (const right of pieces)
      for (const token of ["(", "{", "*", "_", "`", '"', "'", "[", "a", " "]) {
        const input = cursor(left + right, left.length);
        const result = completeInput(input, token);
        if (!result) continue;
        assert.ok(
          result.start >= 0 && result.start <= result.end && result.end <= result.text.length,
        );
        if (result.text !== input.text) assert.ok(result.text.startsWith(left));
      }
  for (const state of [cursor("a", -1), cursor("a", 1, 0), cursor("a", 0, 2)])
    assert.equal(completeInput(state, "*"), null);
  assert.equal(completeInput(cursor(""), "two"), null);
  for (const [open, close] of [
    ["(", ")"],
    ["{", "}"],
    ["*", "*"],
    ["_", "_"],
    ["`", "`"],
    ['"', '"'],
    ["'", "'"],
  ]) {
    assert.ok(open && close);
    const edit = completeInput(cursor("text", 0, 4), open);
    assert.deepEqual(
      edit,
      open === "*" ? cursor("**text**", 2, 6) : cursor(`${open}text${close}`, 1, 5),
    );
    assert.deepEqual(removePair(cursor(open + close, 1)), cursor(""));
    assert.deepEqual(completeInput(cursor(open + close, 1), close), cursor(open + close, 2));
  }
  assert.equal(removePair(cursor("()", 0, 1)), null);
  assert.equal(removePair(cursor("(x", 1)), null);
  assert.equal(removePair(cursor("(")), null);
  assert.deepEqual(completeInput(cursor("word", 0, 4), "*"), cursor("**word**", 2, 6));
  assert.deepEqual(wrapSelection(cursor("", 0), "**"), cursor("**text**", 2, 6));
  assert.deepEqual(completeInput(cursor("**", 1), " "), cursor("* ", 2));
  for (const text of ["**", "__", "``"]) {
    const token = text[0];
    assert.ok(token);
    assert.deepEqual(completeInput(cursor(text, 1), token, true), cursor(text + text, 2));
  }
});

test("list, fence, and indentation edits preserve the unselected remainder", () => {
  for (const source of ["- one", "+ one", "* one", "8) one", "> one", ">> one", "  - [X] one"]) {
    const result = continueLine(cursor(source));
    assert.ok(result);
    assert.ok(result.text.startsWith(`${source}\n`));
    assert.equal(result.start, result.text.length);
  }
  assert.deepEqual(continueLine(cursor("``` ```", 3)), null);
  assert.deepEqual(continueLine(cursor("``````", 3)), cursor("```\n\n```", 4));
  assert.deepEqual(continueLine(cursor("  code")), cursor("  code\n  "));
  assert.equal(continueLine(cursor("one", 0, 1)), null);
  for (const text of ["a\nb", "\ta\n  b", "\n", "a\n"]) {
    const state = cursor(text, 0, text.length);
    for (const size of [2, 4, 8]) {
      const indented = indentLines(state, size, false);
      assert.equal(indentLines(indented, size, true).text, text);
    }
  }
  assert.deepEqual(indentLines(cursor("\ta\n b", 1, 5), 4, true), cursor("a\nb", 0, 3));
});

test("the input adapter respects cancellation, composition, modifiers, and focus exit", () => {
  const body = document.createElement("textarea");
  document.body.append(body);
  let bubbled = 0;
  document.addEventListener("input", (event) => {
    if (event.target === body) bubbled++;
  });
  const deferred: Event[] = [];
  const dispatch = body.dispatchEvent.bind(body);
  const setRange = body.setSelectionRange.bind(body);
  let settingRange = false;
  body.dispatchEvent = (event: Event): boolean => {
    if (settingRange && event.type === "select") {
      deferred.push(event);
      return true;
    }
    return dispatch(event);
  };
  body.setSelectionRange = (start, end, direction): void => {
    settingRange = true;
    try {
      setRange(start, end, direction);
    } finally {
      settingRange = false;
    }
  };
  let selected = -1;
  registerSurface({
    element: body,
    visible: () => true,
    afterLayout: (callback) => callback(),
    focus: () => body.focus(),
    caret: () => null,
    select: (from, to) => {
      selected = from;
      assert.equal(to, from);
    },
  });
  let enabled = true;
  let writable = true;
  let inputs = 0;
  body.addEventListener("input", () => inputs++);
  initInput(
    body,
    () => writable,
    () => enabled,
    () => 4,
  );
  const input = (data: string | null, init: InputEventInit = {}): InputEvent => {
    const event = new InputEvent("beforeinput", {
      data,
      inputType: "insertText",
      cancelable: true,
      ...init,
    });
    body.dispatchEvent(event);
    return event;
  };
  const key = (value: string, init: KeyboardEventInit = {}): KeyboardEvent => {
    const event = new KeyboardEvent("keydown", { key: value, cancelable: true, ...init });
    body.dispatchEvent(event);
    return event;
  };
  assert.equal(input("*").defaultPrevented, true);
  assert.equal(body.value, "**");
  assert.equal(body.selectionStart, 1);
  input("*");
  assert.equal(body.value, "****");
  assert.equal(body.selectionStart, 2);
  assert.equal(inputs, 2);
  assert.equal(bubbled, 2);
  body.value = "*word*";
  body.setSelectionRange(5, 5);
  assert.equal(key("Tab").defaultPrevented, true);
  assert.equal(body.selectionStart, 6);
  assert.equal(selected, 6);
  assert.equal(inputs, 2);
  for (const init of [
    { isComposing: true },
    { cancelable: false },
    { inputType: "insertFromPaste" },
  ])
    assert.equal(input("*", init).defaultPrevented, false);
  assert.equal(input(null).defaultPrevented, false);
  assert.equal(input("x").defaultPrevented, false);
  for (const init of [
    { isComposing: true },
    { ctrlKey: true },
    { metaKey: true },
    { altKey: true },
  ])
    assert.equal(key("Tab", init).defaultPrevented, false);
  assert.equal(key("Shift").defaultPrevented, false);
  enabled = false;
  assert.equal(input("*").defaultPrevented, false);
  assert.equal(key("Tab").defaultPrevented, false);
  enabled = true;
  writable = false;
  assert.equal(input("*").defaultPrevented, false);
  assert.equal(key("Tab").defaultPrevented, false);
  writable = true;
  assert.equal(key("Escape").defaultPrevented, false);
  assert.equal(key("Tab").defaultPrevented, false);
  assert.equal(key("Tab").defaultPrevented, true);
  body.value = "()";
  body.setSelectionRange(1, 1);
  key("Escape");
  assert.equal(key("Backspace").defaultPrevented, true);
  assert.equal(body.value, "");
  body.value = "- one";
  body.setSelectionRange(5, 5);
  key("Enter");
  assert.equal(body.value, "- one\n- ");
  key("x");
  assert.equal(body.value, "- one\n- ");
  key("Escape");
  key("Shift");
  assert.equal(key("Tab").defaultPrevented, false);
  key("Escape");
  key("ArrowLeft");
  assert.equal(key("Tab").defaultPrevented, true);
  for (const position of [1, 2]) {
    body.value = "_";
    body.setSelectionRange(0, 1);
    input("_");
    assert.equal(body.value, "___");
    body.setSelectionRange(position, position);
    input("_");
    assert.equal(body.value, "___");
  }
  body.value = "***";
  body.setSelectionRange(1, 1);
  input("x");
  input("*");
  input("*");
  assert.equal(body.value, "***");
  for (const [type, source, from, to, expected] of [
    ["deleteContentBackward", "*word*", 1, 1, "word"],
    ["deleteContentForward", "*word*", 5, 5, "word"],
    ["deleteByCut", "**word**", 0, 2, "word"],
    ["deleteWordBackward", "*word*", 1, 1, "*word*"],
    ["deleteContentForward", "word", 0, 0, "word"],
  ] as const) {
    body.value = source;
    body.setSelectionRange(from, to);
    const event = input(null, { inputType: type });
    assert.equal(event.defaultPrevented, expected !== source);
    assert.equal(body.value, expected);
  }
  body.value = "*word*";
  body.setSelectionRange(5, 5);
  assert.equal(key("Delete").defaultPrevented, true);
  assert.equal(body.value, "word");
  body.value = "*word*";
  body.setSelectionRange(1, 1);
  assert.equal(key("Backspace").defaultPrevented, true);
  assert.equal(body.value, "word");
  for (const event of deferred) dispatch(event);
  registerSurface(null);
  body.remove();
});
