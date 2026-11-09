# Verify the service

Run `mise x -- just check` for the required local gate.
The gate builds the assets before checking Rust, TypeScript, HTTP behavior, browser workflows, dependencies, secrets, and prose.
Native encryption checks cover device sessions, restart locking, original byte ranges, live journals, corrupted storage, incomplete backups, and bounded space reclamation.
Shutdown checks keep an upload body unfinished, send SIGTERM to an owned server process, and require the process and directory lock to exit within six seconds.
The license policy limits `blessing` and `WTFPL` permissions to the pinned `sqlcipher-src` package after checking its bundled license declarations.
Distributed executable outputs include the SQLCipher copyright notice.
Migration fixtures interrupt committed batches, reject changed sources, and preserve search, recovery identities, Trash visibility, pins, metadata, and original bytes.
Browser fixtures exercise enrollment, wrong recovery keys, failed persistent storage, remembered keys after browser and server restarts, and media inside encrypted notes.
Rendered note fixtures select complete Markdown through keyboard commands before clearing, restoring, and continuing input.
Use `mise x -- just scale-profile --encrypted --counts 1000000` for the bounded encrypted capacity campaign.
Install the shared `prose` and `dotfiles` commands from the pinned source revision in the workflow when preparing another machine.
The same commands validate the decision records and generated repository files.

The browser suite uses Chromium, Firefox, WebKit, and mobile device profiles.
Each run uses a separate data directory, and recovery fixtures use separate note identities for each profile.
It checks note operations, draft recovery, delayed responses, stale tabs, and composition events.
Concurrent editing fixtures cover conditional refresh, active draft ownership, hidden-page suspension, remote deletion, and separate encrypted browser profiles.
Empty page checks reject scrolling and persistent formatting controls at desktop, narrow, landscape, and enlarged text sizes.
Keyboard workflows check delimiter completion, selected text formatting, link entry, list continuation, indentation, undo, and focus exit.
Composition fixtures verify that Markdown completion leaves composing input unchanged.
Composition events exercise the app's input protocol without claiming to emulate a phone's native keyboard.
Writing checks verify editable timestamp headings and centered input through wrapping with fixed typing defaults.
Live editing checks cover inline formatting, source selection, composition boundaries, delimiter completion, and original-image insertion.
Regressions type a complete heading into an empty document, compare list positions across selection changes, and ignore queued selection notifications from the backing field.
The selection adapter uses direct calls because the [HTML selection API](https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#dom-textarea/input-setselectionrange) queues native notifications.
The DOM fixture defers those notifications while browser tests exercise the actual editor.
Container checks cover all bullet markers, nested quotes, empty-item exit, literal code, and stable source positions.
Consecutive Enter presses pass through the input adapter before native Android key deferral can schedule a second insertion.
Handled delimiter deletions stop during event capture before CodeMirror can queue another iOS key operation.
Selected source deletions use CodeMirror commands during event capture to preserve the range before native iOS deletion changes the selection.
Regression cases clear complete notes with Backspace and Delete, restore their source with Undo, and repeat after note creation.
Regression cases delete paired syntax, immediately undo, and continue editing across browser profiles.
Centered typing measures the browser's caret after the editor completes layout, with CodeMirror coordinates as a fallback for empty ranges.
Link completion checks cover title filtering, keyboard insertion, cancellation, and stable stored targets.
Formatting checks cover blur, keyboard activation, undo, original code spacing, and stale asynchronous results.
Automatic formatting stops when the writer returns to the editor or changes the selection.
Identity checks cover duplicate titles and heading changes without changing stored addresses.
Slash checks exercise filtering, keyboard activation, original-byte uploads, cancellation, failure, and writing during a pending upload.
Navigation checks return to the same draft after saving completes on a secondary page.
Attachment checks cover inline paste and drop, pending uploads, original bytes, and permanent deletion.
Clipboard fixtures supply transfer data explicitly across browser engines.
Live file insertion passes original files to the upload adapter without cloning browser events.
Explicit-clock storage tests cover recovery expiry, checkpoint coalescing, immutable checkpoint identities, reference return, cleanup failure, and symbolic links.
The editor uses CSS Object Model (CSSOM) View APIs for geometry.
Browser checks establish supported behavior because the [CSSOM View Module](https://www.w3.org/TR/cssom-view-1/) remains a Working Draft.
Browser tests write screenshots under `target/screenshots`.
The style gate parses browser style sheet rules and tests rejecting fixtures for each prohibited visual effect.
The gate checks generated CodeMirror vendor styles through rendered computed styles.
The authored style sheet remains subject to the palette scanner.
Icon checks inspect visible controls across note, graph, Trash, and attachment workflows and reject a missing-icon fixture while accepting its labeled counterpart.
SVG bounds include half the stroke width, and a clipped-path fixture must fail the check.
The checker uses the initial values in the CSS drafts for [background images](https://www.w3.org/TR/css-backgrounds-3/#the-background-image) and [text decoration colors](https://www.w3.org/TR/css-text-decor-3/#text-decoration-color-property).

The [Web Content Accessibility Guidelines](https://www.w3.org/TR/WCAG22/) govern applicable keyboard, naming, focus, reflow, resizing, and status behavior.
Browser tests check 320 CSS pixel reflow, 200 percent text enlargement, control labels, and automated accessibility rules.
They also check text spacing overrides, landscape input, and the notice when JavaScript is off.
Native controls preserve keyboard and assistive technology semantics.
The graph provides keyboard movement, stops continuous animation when hidden or settled, and honors reduced-motion preferences.
Timed tasks and authentication criteria have no matching features in the app.
Black and white styling, 18 CSS pixel text, and 48 CSS pixel targets are owner preferences.
Automated results don't establish behavior in an untested assistive technology.

## Browser performance

Run `mise x -- just browser-performance --dry-run` to inspect the bounded workload.
Run `mise x -- just browser-performance` to build and inspect the embedded production assets with Lighthouse.
The command uses two CPUs, 2 GiB of memory, no swap, 256 processes, no network, and a container deadline of 590 seconds.
Desktop and mobile profiles each cover an empty note, a note with links, tasks and code, and the Trash page.
The mobile profile uses sixfold CPU slowdown and Lighthouse's simulated mobile network.
The gate requires performance of at least 95 and accessibility and best practices of 100.
The search engine optimization score must reach 90.
First Contentful Paint must stay within 1800 milliseconds, Largest Contentful Paint within 2500 milliseconds, Total Blocking Time within 150 milliseconds, and Cumulative Layout Shift within 0.05.
Native HTML and JSON reports remain under a distinct `target/browser-performance-*` directory.
The candidate release gate runs the same command after mutation testing.

The regular browser suite limits initial decoded JavaScript to 420 KiB and transferred JavaScript to 180 KiB.
PDF rendering, formatting, syntax highlighting, graph simulation, and version differences load on demand.
HTTP fixtures verify compression, conditional requests, immutable fingerprinted chunks, and private pages retaining `no-store`.
Source files and retired settings and export routes remain unavailable.
Chromium tests measure twelve edits under sixfold CPU slowdown and require the ninety-fifth percentile through the following rendering frame to stay below 200 milliseconds.
Other engines run the same editing behavior without claiming support for Chromium's CPU emulation.
Lighthouse's [performance score](https://developer.chrome.com/docs/lighthouse/performance/performance-scoring) and synthetic latency budgets cover the specified fixture and emulation.
Physical devices and networks can differ from the laboratory conditions.

## Release checks

Run `mise x -- just editing-assurance --dry-run` to inspect the mutation inventory and validate the runner.
Run `mise x -- just editing-assurance` for the bounded [Stryker](https://stryker-mutator.io/docs/stryker-js/configuration/) campaign over production input and decoration rules.
The same campaign runs at the candidate release gate.
It uses two CPUs, 2 GiB of memory, no swap, 256 processes, no network, and a time limit of 900 seconds.
Native reports remain in a distinct directory under `target` for each run.
The gate rejects survivors, uncovered mutations, timeouts, runtime errors, ignored mutations, empty inventories, and reports from changed source.
Compile errors remain separate from mutations detected by tests.
The mutation command uses [Node's shared-process mode](https://nodejs.org/api/cli.html#--test-isolationmode), so cancellation can't leave test children running.
Normal coverage checks keep independent test processes.
The isolated tool package pins TypeScript 6 for Stryker's compiler API while the normal gate uses the project's TypeScript 7 compiler.
The normal gate checks lines, branches, and functions in the same production rules with Node's coverage reporter.
Generated input cases cover Unicode, nesting, escaped markers, incomplete syntax, delimiter completion, selection bounds, and indentation round trips.
Coverage and mutation results cover those rules.
Browser geometry, media, storage, and native composition keep their integration checks.

Run `mise x -- just release-check` for a candidate release.
The command first runs the local gate, then uses the bounded Linux proof container.
The container has 4 GiB of memory, no swap, two CPUs, 256 processes, and a safety limit of 900 seconds.
An exhausted limit leaves the result inconclusive and blocks release.
Proofs and fuzzing use fixed input bounds and run counts.

The proof crate imports the production filename and revision rules.
The translator parses the production TypeScript syntax tree into equivalent Rust boolean expressions.
The translator accepts only explicit boolean parameters, boolean operators, and a single pure return.
Tests reject unsupported syntax and effects before a proof can run.
Kani checks the full admitted boolean domains and filename inputs up to the production length limit.
Filesystem durability and browser networking remain external boundaries covered by integration and fault-injection tests.
Single-note reads synchronize the file and directory before acknowledging content after an ambiguous save response.
The gate runs every discovered proof harness, 20000 seeded fuzz inputs, and the configured Rust rule mutations.
The [formal assurance guide](formal-assurance.md) describes the production decisions, temporal models, input bounds, and rejected verification results.
Run `mise x -- just proofs --dry-run` to inspect the proof allocation or `mise x -- just proofs` to run the isolated proof gate.
Name harnesses cover their admitted length limits.
The link scheme harness covers 128 byte inputs.
The attribute escape harness covers 64 byte UTF-8 inputs.
Parser fixtures check escaped HTML, image restrictions, links, tags, task offsets, and heading identities.
HTTP fixtures verify original attachment bytes without a bulk export route.
Browser fixtures check native text edits, delayed preview responses, dialogs, retired routes, and recovery.
Every editing fixture exercises the visible Live preview editor and observes its source adapter without switching views.
Parser and browser behavior cross dependency and DOM boundaries and remain integration checks.
Named regions use direct labels matching their headings, following the [W3C region examples](https://www.w3.org/WAI/ARIA/apg/patterns/landmarks/examples/region.html).
A direct label avoids a pinned Firefox crash when the preview region refers to its heading by ID.
The browser suite runs the complete selected accessibility rule set and rejects an unnamed button fixture.
Media checks decode an image, play a recording, render PDF pixels and text, follow links, and select PDF pages.
The viewer follows the [HTML media contract](https://html.spec.whatwg.org/multipage/media.html#media-elements) and the [PDF.js rendering API](https://mozilla.github.io/pdf.js/examples/).
Media descriptions support transcripts supplied by the owner.
The service doesn't transcribe private uploads.
Original user content needs its own captions and alternatives under the [Web Content Accessibility Guidelines media criteria](https://www.w3.org/TR/WCAG22/#time-based-media).
Retired settings, appearance preferences, text counts, heading controls, and alternative views stay absent.
Fields have [associated labels](https://www.w3.org/WAI/tutorials/forms/labels/).
Select controls use [CSS appearance](https://www.w3.org/TR/css-ui-4/#appearance-switching) to keep their dimensions and icon spacing on mobile Safari.
The CSS specification remains a Working Draft, and browser checks verify the control geometry and native selection behavior.
The suggestion popup keeps focus in the editor using the [W3C combobox keyboard model](https://www.w3.org/WAI/ARIA/apg/patterns/combobox/).
Firefox fixtures can't focus native anchors beneath an editable ancestor.
Live links use focusable wrappers with [link activation](https://www.w3.org/WAI/ARIA/apg/patterns/link/) while retaining their native anchor URLs and pointer context menus.
Keyboard navigation opens the target note and returns focus to its editor.
Dialog and page checks verify the name and upper-right placement of close controls.
Heading checks enforce the owner's single-title and continuous-rank policy while preserving literal code and Undo.
Matched-delimiter and fence checks preserve contents, unrelated blocks, and native Undo.
Highlighting fixtures cover language aliases, common grammars, a deferred grammar, literal HTML, and an unknown language.
Trash uses the [single-select listbox keyboard model](https://www.w3.org/WAI/ARIA/apg/patterns/listbox/) with indexed paging, selection, preview, and restoration through browser history.
Version comparisons bound computation and output, preserve Unicode, and insert source text without interpreting HTML.
Explicit-clock storage fixtures preserve a session's starting state, checkpoint idle or structural changes, and suppress duplicate or empty snapshots.
File-link checks hold the save request until validation completes and reject stale results.
Repair preserves unrelated edits, while deliberate removal dismisses the warning.
Line-break fixtures cover plain text, links, explicit breaks, paragraphs, and literal code.

Run `mise x -- just artifacts` to build Linux amd64 and arm64 candidates under `dist`.
The container build uses the same source and embedded browser assets.
Candidate builds don't publish a release or an image.

Explore notes checks cover topic exploration, disconnected-note search, node movement, pan, zoom, keyboard access, and linked creation.
Graph fixtures preserve complete titles, honor reduced motion, and verify that closing stops simulation.
Pure graph fixtures check bounded neighborhoods and reject edges to missing notes.
The sample generator checks counts, repeatability, unique titles, internal destinations, and refusal to replace existing data.
The scale generator checks random access across a million-page collection without keeping every note in memory.
Mixed fixtures cover isolated and terminal notes, duplicate titles, and intentional missing destinations.
Filesystem checks reject an undeclared missing destination.
Local socket checks reject truncated responses, missing lengths, and streaming responses, and identify HTTP failures and response budget overruns.
The optional `scale-profile` command measures real generated collections with bounded resources.
Dataset verification and latency gates must both pass.
The separate `test:demo` browser command exercises a generated collection and ordinary pointer selection.
Graph labels never intercept pointer input, while native buttons keep their target size and `accessible name`s.
Internal-link fixtures check relative destinations, escaped Unicode titles, explicit aliases, literal code, recovery records, and stale revisions.
Graph controls follow native [button keyboard behavior](https://www.w3.org/WAI/ARIA/apg/patterns/button/) and keep fixed target sizes during zoom.

## Scale observations

The following baseline predates the indexed implementation.
On 2026-11-09, the default mixed profile with seed 42 completed its three dataset stages in 90.6 seconds on OrbStack.
The campaign used the documented two CPU and 2 GiB limits.
The million-page dataset contained 5676337 parsed links, including 9135 intentional missing destinations.
It included 29791 isolated notes, 86779 notes without outgoing links, 18557 repeated nonempty titles, and 10032 pages without a title heading.
The largest connected component contained 921714 notes, while the most popular note had 66990 incoming links.
The million-page dataset occupied 4199305216 bytes in allocated file blocks.

With 10000 pages, search, listing, and connections completed in 232, 224, and 453 milliseconds.
With 100000 pages, search completed in 2350 milliseconds.
Listing returned a declared 34164782 byte response after 2548 milliseconds, and connections returned 28845751 bytes after 5020 milliseconds.
Both collection responses exceeded the 4 MiB budget before the profiler downloaded their bodies.

With 1000000 pages, startup completed in 9331 milliseconds and opening the entry note completed within one millisecond.
Search, listing, and connections each exceeded the request deadline of 15 seconds.
Requests ran serially after generation and a full file audit in the same service process.
Timed out storage work can keep the store mutex and delay later requests, so the observations include queueing.
The measurements expose collection scans and full collection responses as limits on practical use at larger sizes.
The successful profiling command doesn't establish million-page support.

## Indexed capacity checks

The indexed implementation bounds search pages, title suggestions, and graph neighborhoods instead of transferring the collection.
Storage fixtures check Unicode substring semantics, exact title ranking, cursor validation, pagination, interrupted writes, index reconciliation, and queued title updates across restarts.
Recovery checks return the newest 50 records without opening unrelated bodies.
Trash clearing remains durable and leaves later deletions restorable.
Attachment collection checks bounded batches, original bytes, returning references, fragments, and recovery retention.
Repeated-heading and task fixtures validate unique heading identifiers and UTF-16 positions without repeated prefix scans.
A browser fixture edits an 840 KB note with 35000 repeated headings while bounding rendered elements.
PDF and audio fixtures preserve media instances across transient live rendering changes.
Layout updates wait for a rendering frame instead of changing observed sizes inside a [Resize Observer notification](https://www.w3.org/TR/resize-observer/#html-event-loop).
The observer specification remains a Working Draft and browser checks cover the notification behavior.

On 2026-11-09, an extreme campaign used 1000000 Markdown files, 999999 incoming links into one hub, and 1000000 recovery records for that hub.
The runtime retained two CPUs, 2 GiB of memory, no swap, and a campaign limit of 900 seconds.
Initial index construction completed in 251214 milliseconds and a warm restart completed in 8357 milliseconds.
Twenty serial measurements per endpoint kept search and list ninety-fifth percentiles below one millisecond.
The warm graph ninety-fifth percentile was 58 milliseconds with a 42164 byte response.
Reading and validating 50 recovery sources completed below one millisecond.
Saving the hub took 15 milliseconds, processing unchanged aliases took 2 milliseconds, saving another note took 11 milliseconds, and an eight-record cleanup batch took 13 milliseconds.
The former bulk export operation streamed into a sink in 9008 milliseconds.
Recovery records shared immutable bytes across 4096 variants with separate record directories and identifiers.
The measurements cover the synthetic fixture and local environment rather than every possible note collection.

The mixed million-page collection with 5676337 parsed links passed the same indexed endpoint gates.
Its graph ninety-fifth percentile was 11 milliseconds, overview was 53 milliseconds, and eight simultaneous clients completed 32 mixed requests with a 68 millisecond ninety-fifth percentile.
The largest measured response was 55745 bytes.
The retired bulk export operation took 3473 milliseconds into a sink.
Opening the reconciled catalog took 10702 milliseconds.

## Encrypted capacity checks

On 2026-11-09, the encrypted mixed profile with seed 42 completed the million-note capacity gate on OrbStack.
The run resumed the earlier partial fixture, completed preparation in 69958 milliseconds, and finished all observations in 71893 milliseconds.
The container retained two CPUs, 2 GiB of memory, no swap, no network, and the deadline of 900 seconds.
The approved allocation budget was 12 GiB, and the completed data directory occupied 9165807616 bytes in allocated blocks.
The fixture stored original note bodies and indexes inside SQLCipher and exercised authenticated HTTP requests without a browser index replica.

The profiler reports integer milliseconds.
Each serial endpoint ran 20 times.
Search reported 1 millisecond at the ninety-fifth percentile and 20 milliseconds for its slowest request.
Short, Unicode, negative, and absent searches each reported a ninety-fifth percentile below one millisecond.
The neighborhood graph reported 11 milliseconds at the ninety-fifth percentile and 25 milliseconds for its slowest request.
The overview graph reported 59 milliseconds at the ninety-fifth percentile and 63 milliseconds for its slowest request.
Eight simultaneous clients completed 32 mixed requests.
The ninety-fifth percentile and slowest response both reported 109 milliseconds.
The largest measured response was 55745 bytes.
Every measured endpoint kept its ninety-fifth percentile within 500 milliseconds and every response within one second.
The measurements cover the synthetic fixture after preparation in the measured container and local environment.
