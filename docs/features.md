# Write connected notes

## Writing

Opening the service shows the last note or an unwritten page.
Explore notes opens a graph of linked notes.
New note starts with an editable local timestamp heading and space to write.
The initial page fits the viewport without scrolling.
The page has no brand heading, sidebar, or formatting toolbar.

Live preview is the only editor.
Large notes display Markdown source automatically to keep editing responsive.
Emphasis, strong emphasis, strikethrough, and inline code render while neighboring text remains editable.
The formatted expression containing the cursor or selection reveals its delimiters.
Heading and list geometry stays stable when the cursor moves.
Unordered lists render `-`, `+`, and `*` as bullets.
Quotes use an inset rule, and fenced code keeps literal text in a monospace block.
Moving into a container marker reveals its original syntax without moving the following lines.
Attachments, links, and complex interactive blocks use the server's sanitized rendering outside the active block.
Clearing a note leaves an empty document without hidden markers or prompts.
The editor preserves automatic saving, browser draft recovery, revision checks, and original Markdown source.
Tabs keep independent drafts, and clean notes refresh changes made in another tab or device.
Competing edits preserve local writing and offer recovery without overwriting an acknowledged revision.
The page has no heading gutters, folding controls, or title-editing icon.
Code blocks highlight explicit language names and aliases with bundled [Highlight.js grammars](https://highlightjs.readthedocs.io/en/latest/api.html).
Common grammars load only when a code block needs them, and other stock grammars load on demand.
Unknown languages remain literal text.
Highlighting uses weight and emphasis within the black-and-white palette.
Highlighting stops when source exceeds 65536 UTF-16 code units.

The parser supports [CommonMark](https://spec.commonmark.org/0.31.2/) with tables, task lists, strikethrough, and footnotes from [GitHub Flavored Markdown](https://github.github.com/gfm/).
Raw HTML stays inert and remote images appear as descriptions without a network request.
A single newline renders as a line break, matching [GitHub comments](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax#line-breaks).
Blank lines separate paragraphs, and fenced code keeps its original newlines.
Local images, recordings, videos, and PDFs appear in rendered blocks.
Task checkboxes update the original Markdown through the same save protocol as typing.

Live preview formats Markdown automatically after editing.
Leaving the editor formats Markdown with bundled [Prettier standalone](https://prettier.io/docs/browser).
Alt+Shift+F formats immediately without a formatting button.
Formatting preserves prose wrapping and fenced code contents.
The first heading becomes the single level-one heading, and later headings can't skip a descending rank.
The stricter heading policy follows the owner's instruction beyond [CommonMark's syntax](https://spec.commonmark.org/0.31.2/#atx-headings).
Notes exceeding 65536 UTF-16 code units keep their source without automatic formatting to bound foreground work.
Undo restores the previous source.
A delayed formatting result can't replace newer text, another note, or composing input.

Typing completion pairs delimiters, wraps selected text, and continues lists.
Selecting text and pressing Ctrl or Command with B wraps bold text.
Use Ctrl or Command with the italic shortcut, `I`, to wrap italic text.
Backspace removes an empty pair.
Deleting a Markdown delimiter removes its matching delimiter while preserving the enclosed text.
Deleting either matched code fence also removes its counterpart and language metadata.
Escaped punctuation, literal code, and unmatched delimiters remain unchanged.
Enter removes an empty list marker or exits the innermost quoted container.
Markdown markers inside code blocks remain literal.
Tab moves from a link label to its destination and then past the link.
Shift with Tab returns to the label.
Tab also moves past paired delimiters.
Tab indents lines outside paired expressions.
Shift with Tab removes indentation.
Escape followed by Tab moves focus out of the editor.

In Live preview, Alt with ArrowUp or ArrowDown moves the current line or selected lines.
Add Shift to duplicate the lines.
Ctrl or Command with Shift and K deletes the current line.
Undo restores line edits through the same history as typing.

Typewriter mode keeps the active line near the viewport center after typing reaches that position.
Wrapped lines and visible viewport changes affect cursor measurement.
Manual scrolling remains available between edits.

Type slash at the start of a body line to choose an insertion.
Filter by typing and choose with the arrow keys and Enter or Tab.
Escape leaves the original command.
Commands insert headings, lists, tasks, quotations, code, links, tables, and dividers.
The single `/file` command opens an unrestricted file picker.
Uploaded images, audio, video, and PDFs use their matching embedded viewers automatically.
Cancellation and upload failure keep the command.
An upload appends its links without replacing text written while the request was pending.
If the active note changes, returning to the original note inserts its completed uploads.
Commands stay inactive during composition and inside fenced code.

## Titles and connections

The collection is flat and uses headings as titles.
Notes without headings use their first nonempty line or Untitled note.
New notes receive stable internal identifiers.
Editing a heading changes the visible title without moving the stored file or changing link targets.
Inline link labels matching the preceding title update across notes, while explicit aliases remain unchanged.
Large updates continue in durable background batches without delaying the initiating save.
Affected notes receive recovery checkpoints and fresh revisions.
Notes can share a title.
Existing files keep their paths and links without migration.
The interface has no note filename, folder, tag, pin, or template controls.

Explore notes shows real link directions without synthetic index notes.
Select a topic to explore its neighbors, then use Open note to read or edit.
Drag nodes to arrange them, drag the background to pan, and use the wheel or zoom controls to change scale.
Arrow keys pan.
Shift with an arrow moves the focused node.
Fit graph restores the visible bounds, and Overview shows the collection without selecting a topic.
Each view contains at most 120 notes and displays its count.
Smaller viewports show fewer nodes and show direct neighbors first.
Node buttons keep their target size, while labels appear where space permits and never intercept pointer input.
The full title remains available through the `accessible name`, native tooltip, and selected-note heading.
Visible labels wrap instead of shortening their titles with an ellipsis.
The [D3 force simulation](https://d3js.org/d3-force/simulation) separates linked nodes and responds to dragging.
Simulation stops after settling, after 1.8 seconds without interaction, while hidden, and when the graph closes.
Reduced-motion preferences stop continuous animation.
Rendering uses one scheduled frame and reuses existing edges, with at most 600 remembered node positions.
Search covers the full collection, including disconnected notes.
Creating a note from the graph adds a link to the selected topic.

Typing `[` suggests existing note titles at the cursor.
Arrow keys select a suggestion, Enter or Tab inserts it, and Escape preserves the original text.
Suggestions keep internal link targets and stay inactive in code, images, existing destinations, and composing input.
Existing relative Markdown links and wiki links remain valid.
A missing target reports that the linked note is unavailable.
Direct note URLs work inside the tailnet.

## Files and recovery

Files attach through slash commands, image paste, and file drop while writing.
The app has no separate upload or file management page.
Each attachment can contain at most 64 MiB.
The service keeps existing names, metadata, and original bytes readable.

Changing an inline attachment destination checks the target before sending the note's save.
An unavailable destination shows a warning and a repair action that preserves other edits.
Removing a complete link expresses deletion and needs no warning.
Original files remain available through retained history.
Plaintext startup reconciles source files, recovery records, and attachment references.
Encrypted storage keeps payloads and references in one SQLCipher database.
Bounded background batches expire recovery records and check indexed live and recovery references before deleting eligible originals.
An attachment becomes eligible for deletion after seven days without references.
A reference returning during the grace period resets its timer.
Unreadable sources stop attachment deletion.
File options can permanently delete an original immediately after confirmation.
Retained versions and other notes then lose access to the deleted original.

Images decode in the browser.
Audio and video use native controls and byte ranges for seeking.
Playback depends on installed codecs.
PDF pages use bundled PDF.js with page selection, zoom, extracted text, passwords, and links.
Editing an earlier paragraph preserves an unchanged media widget and its selected PDF page.
Open and Download keep the original file available if decoding fails.
Original files and compatible metadata remain inside encrypted storage or the legacy attachment directory.

Version history keeps at most 50 recovery points per note for up to seven days.
Continuous edits keep the session's starting text without replacing its recovery identity.
A pause of 90 seconds or a session lasting five minutes starts another checkpoint.
Heading or link changes and replacements of at least 1024 bytes also create checkpoints.
An unchanged source doesn't create another checkpoint, and an empty starting document needs no checkpoint.
Viewing a version compares its text with the current note and summarizes the lines that restoration adds and removes.
Comparisons have a 40 millisecond computation budget and truncate output after 200 lines or 16 KiB.
Large or excessive differences keep access to the full saved content without computing a comparison.
Restoration checks the current revision and preserves the replaced text.
Trash keeps deleted notes for seven days.
Deleting the open note returns to a visited live note in the current tab or another recent note.
If none remain, the editor shows an unsaved new note.
The deleted note leaves the active address and startup selection.
Recovery preserves unsaved changes to a deleted note and skips browser drafts that only repeat its saved text.
The next note receives focus after the menu closes, following [W3C dialog workflow guidance](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/#keyboard-interaction).
A later navigation or new draft takes precedence over a pending replacement.
Trash opens a dedicated page with selectable rows, capture times, preview, and restore.
Arrow keys move selection, Enter previews the selected note, and paging fetches 50 records using indexed cursors.
Restoring a name collision creates a separate internal identity without replacing the existing note.
Empty trash removes every deleted note after confirmation.

The app has no import, bulk export, individual Markdown download, printing, duplication, or find and replace operation.
The editor supports ordinary Markdown copy and paste.
Long-term backups remain filesystem operations outside the app.
Legacy plaintext installations also support external Git history.

## Paper and controls

The paper has a fixed reading width of at most 43 rem.
Text uses 18 CSS pixels with a line height of 1.75.
Browser zoom changes text size through the browser's normal shortcuts.
The app has no settings page, custom font controls, appearance presets, or text counts.
Retired browser preferences don't change the page.

Interactive controls have meaningful icons alongside their labels.
Fields have adjacent icons and native keyboard selection.
Icons share their action target, and assistive technology ignores redundant icons.
Close uses a named cross icon at the upper right.
