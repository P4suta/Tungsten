# Verify the service

## Linux app

Run `mise x -- just linux source` on every development host.
Run `mise x -- just linux check` on Linux with GTK 4.18 or later, libadwaita 1.9 or later, Xvfb, D-Bus, desktop-file-utils, AppStream, and a Japanese system font.
The gate checks strict Clippy, durable save decisions, real-server contracts, metadata, and native GTK interactions in isolated app directories.
GTK workflows cover Undo and Redo, history restoration, composition guards, storage failure, navigation, enlarged text, and narrow layouts.
Snapshots wait for GTK's rendered frame instead of assuming a fixed rendering delay.
Separate processes exercise ordinary and High Contrast appearance with libadwaita's supported debug setting.
Actual input methods, the screen keyboard, screen readers, and desktop integration require isolated sessions with `XDG_CURRENT_DESKTOP=GNOME` or `KDE`.

Run `mise x -- just linux bundle --arch x86_64` and `mise x -- just linux bundle --arch aarch64` with Flatpak, flatpak-builder, OSTree, Xvfb, and `org.gnome.Platform//51` and `org.gnome.Sdk//51` installed for the selected architecture.
Each command prefetches locked sources, builds offline, installs its bundle, and checks launch.
The [Linux app guide](linux-apps.md) describes recovery, system design, sandbox permissions, and local distribution.
Production save decisions also join the existing candidate proof and mutation gates.

## Shared checks

Run `mise x -- just check` for the required local gate.
The gate builds the assets before checking Rust, TypeScript, HTTP behavior, browser workflows, dependencies, secrets, and prose.
Native storage checks cover atomic rollback, revision conflicts, original byte ranges, corrupted storage, incomplete backups, and bounded space reclamation.
The directory lock explicitly releases ownership under the [Rust file locking contract](https://doc.rust-lang.org/std/fs/struct.File.html#method.unlock) after database closure.
Regression checks keep a duplicated descriptor open across store replacement and verify that closing the descriptor preserves the replacement store's lock.
Portable archive checks preserve original bytes, recovery records, workspace membership, counters, and pending maintenance while rebuilding indexes.
Restore failures preserve existing destinations.
Shutdown checks keep an upload body unfinished, send SIGTERM to an owned server process, and require the process and directory lock to exit within six seconds.
The license policy permits SQLite's public-domain dedication through its `blessing` identifier.
Run `mise x -- cargo xtask secrets` to check working files and Git history.
Hosted checkouts include complete history, and secret checks reject shallow repositories.
Fixtures verify detection in working files and in earlier commits after a later commit removes the file.
Dataset fixtures resume interrupted committed batches without replacing saved notes.
Browser fixtures exercise writing without setup, unavailable IndexedDB, independent profiles, server restarts, and original media.
Rendered note fixtures select complete Markdown through keyboard commands before clearing, restoring, and continuing input.
Use `mise x -- just scale-profile --counts 1000000` for the bounded database capacity campaign.
Install the shared `prose` and `dotfiles` commands from the pinned source revision in the workflow when preparing another machine.
The same commands validate the decision records and generated repository files.
Project tool pins include the Vale version required by the shared prose checker.
Update Vale together with the shared checker revision and its required version.
Hosted checks run the documentation gate before setting up browsers.

The browser suite uses Chromium, Firefox, WebKit, and mobile device profiles.
Shortcut fixtures choose Control or Meta from the browser's platform and device identity.
Shifted letter shortcuts send uppercase characters under the [Playwright keyboard contract](https://playwright.dev/docs/api/class-keyboard#keyboard-press).
CI stops after the first browser failure, names each test, and retains failure traces for seven days.
Each run uses a separate data directory, and recovery fixtures use separate note identities for each profile.
It checks note operations, draft recovery, delayed responses, stale tabs, and composition events.
Concurrent editing fixtures cover conditional refresh, active draft ownership, hidden-page suspension, remote deletion, and separate browser profiles.
Empty page checks reject scrolling and persistent formatting controls at desktop, narrow, landscape, and enlarged text sizes.
Sidebar checks cover independent visibility, reserved writing space, fixed pane bounds, unavailable storage, keyboard return, and unchanged drafts.
Every wide-screen sidebar combination preserves the paper's bounds, writing width, scroll position, and source selection at four viewport widths.
Compact checks return from navigation and context to the same reading position after centered typing has settled.
Workspace contracts check current-schema initialization, restart persistence, bounded pagination, concurrent additions, rejected stale metadata, rename cascades, and membership cleanup.
Catalog contracts reject unsupported format markers and incomplete recovery records or search cursors.
Browser checks create, rename, and remove workspaces, keep selections independent across tabs, and retry failed membership without duplicating notes.
Delayed deletion checks preserve newly opened navigation and keyboard focus.
Deletion fault injection waits for the linked note before disrupting collection reads.
Navigation uses the [W3C disclosure pattern](https://www.w3.org/WAI/ARIA/apg/patterns/disclosure/) with native links and buttons.
The pane grid uses separate scroll containers and shows one pane on narrow screens to preserve [reflow](https://www.w3.org/WAI/WCAG22/Understanding/reflow.html).
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
Black and white styling, 18 CSS pixel text, and 48 CSS pixel targets are personal preferences.
Automated results don't establish behavior in an untested assistive technology.

## Database capacity

The ordinary SQLite mixed campaign passed with 1000000 notes and 5676337 stored links.
Startup took 101 milliseconds.
Across twenty samples per endpoint, the largest search ninety-fifth percentile was two milliseconds and Overview reached 69 milliseconds.
Eight concurrent clients reached a ninety-fifth percentile of 95 milliseconds.

The extreme campaign passed with 1000000 notes, 999999 incoming links to one hub, and 1000000 recovery records.
Reopening took two milliseconds, hub saving took five milliseconds, and bounded cleanup took nine milliseconds.
Both campaigns used two CPUs, 2 GiB of memory, no swap, and a 10 GiB storage budget in the local Linux container.
The test operator removed the generated campaign volumes after verification.
The observations describe seeded synthetic data and the measured host, without establishing latency on every physical device.

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
Chromium tests load 50 sidebar notes, measure twelve edits under sixfold CPU slowdown, and require the ninety-fifth percentile through the following rendering frame to stay below 200 milliseconds.
Other engines run the same editing behavior without claiming support for Chromium's CPU emulation.
Lighthouse's [performance score](https://developer.chrome.com/docs/lighthouse/performance/performance-scoring) and synthetic latency budgets cover the specified fixture and emulation.
Physical devices and networks can differ from the laboratory conditions.

## Memory footprint

Run `mise x -- just memory-profile --dry-run` to inspect the workload.
Run `mise x -- just memory-profile` for a Linux container with one CPU, 64 MiB of memory, no swap, 32 processes, no network, and a 180-second deadline.
The workload seeds 5000 notes, requests compressed browser assets, repeats indexed search and graph reads, saves a near-limit note with 35000 headings, and round-trips three 64 MiB files.
Uploads and downloads use bounded client buffers, and the gate checks every returned file byte.
The container owns its temporary dataset and removes the anonymous volume on exit.
The server's resident high-water mark must remain within 24 MiB.
Missing, malformed, zero, and excessive measurements fail.
Linux process measurements and total container memory remain distinct because the container also holds the runner and filesystem cache.
[Docker statistics](https://docs.docker.com/reference/cli/docker/container/stats/) subtract inactive file cache, so their display isn't a process resident memory measurement.
The release gate runs the same memory workload.
It also runs the bounded SQLite storage regression workload.
Full cross-engine comparisons remain manual.

## Storage characterization

`mise x -- just storage-profile --engine all --case regression` compares durable note, search, file, recovery, and restart operations.
Small cross-engine contract tests run in the normal Rust test suite.
The shared-index SQLite control runs the same contracts as redb and Fjall.
Packed Unicode key checks compare independent substring windows and check width and scalar boundaries.
Ordered search checks cover exact formatted titles, negative terms, filters, pins, changes, and bounded results.
Shared contracts cover empty path and tag filters, negative filters, and bare negation markers.
Fixture checks cross batch boundaries and preserve editing, recovery, and reopening.
Measurement checks reject missing phases, incorrect operation counts, incomplete outcomes, incompatible baselines, and allocation overruns.
Reports record source identity, container identity, environment, input revision, and durability settings.
Repetition checks require complete engine groups and reject mixed source or input identities before reporting run-level variation.
Use repeated runs on the same machine before attributing a timing change to an engine.

The Fjall comparison adapter requires version-scoped license exceptions for `varint-rs` under [0BSD](https://spdx.org/licenses/0BSD.html) and `xxhash-rust` under [Boost Software License 1.0](https://spdx.org/licenses/BSL-1.0.html).
The exceptions cover only the named Fjall dependencies and versions.

Storage fixtures check streamed hashes, rollback on short and failed readers, unlinked private staging, and restart persistence.
HTTP fixtures check interrupted streams, declared lengths, JSON limits, upload-slot cancellation, and editing while an upload waits for bytes.
Chromium and mobile Chrome collect [heap metrics](https://chromedevtools.github.io/devtools-protocol/tot/Performance/#method-getMetrics) after garbage collection while switching 24 populated notes.
The gate limits retained JavaScript heap to 16 MiB, growth to 2 MiB, DOM growth to 200 nodes, and listener growth to 30.
Reading and navigation must leave formatter chunks unloaded.
Other engines keep their workflow checks without claiming support for Chromium heap instrumentation.

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
The command first runs the local gate, then runs native stages in bounded Linux containers.
Each native stage has 4 GiB of memory, no swap, two CPUs, 256 processes, and a deadline of 890 seconds.
The host needs 12 GiB of free space before image creation and 8 GiB before each native container starts.
Proofs, fuzzing, server rule mutations, and Linux model mutations run sequentially in separate containers and keep separate reports.
Containers write to private anonymous volumes, and the runner copies reports to the host before removing its containers and volumes.
Reports remain inside a private directory under `CARGO_TARGET_DIR` when set, or the project's `target` directory otherwise.
The server rule mutation check runs the dedicated domain contract tests without server or storage features.
The normal gate also runs the complete server contract suite.
An exhausted limit leaves the result inconclusive and blocks release.
Proofs and fuzzing use fixed input bounds and run counts.

The proof crate imports the production filename and revision rules.
The translator parses the production TypeScript syntax tree into equivalent Rust boolean expressions.
The translator accepts only explicit boolean parameters, boolean operators, and a single pure return.
Tests reject unsupported syntax and effects before a proof can run.
Kani checks the full admitted boolean domains and filename inputs up to the production length limit.
Filesystem durability and browser networking remain external boundaries covered by integration and fault-injection tests.
Note writes commit source, recovery records, and derived indexes in one durable SQLite transaction.
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
The gate rejects child frames and uses the [documented analysis mode](https://github.com/dequelabs/axe-core-npm/blob/develop/packages/playwright/error-handling.md#axebuildersetlegacymodelegacy-boolean) that runs in the tested page, avoiding a second WebKit automation window.
Media checks decode an image, play a recording, render PDF pixels and text, follow links, and select PDF pages.
The viewer follows the [HTML media contract](https://html.spec.whatwg.org/multipage/media.html#media-elements) and the [PDF.js rendering API](https://mozilla.github.io/pdf.js/examples/).
Media descriptions support supplied transcripts.
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
Heading checks enforce the single-title and continuous-rank policy while preserving literal code and Undo.
Matched-delimiter and fence checks preserve contents, unrelated blocks, and native Undo.
Highlighting fixtures cover language aliases, common grammars, a deferred grammar, literal HTML, and an unknown language.
Trash uses the [single-select listbox keyboard model](https://www.w3.org/WAI/ARIA/apg/patterns/listbox/) with indexed paging, selection, inline preview, and restoration inside navigation.
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
Stored-body audits reject an undeclared missing destination.
Local socket checks reject truncated responses, missing lengths, and streaming responses, and identify HTTP failures and response budget overruns.
The optional `scale-profile` command measures real generated collections with bounded resources.
Dataset verification and latency gates must both pass.
The separate `test:demo` browser command exercises a generated collection and ordinary pointer selection.
Graph labels never intercept pointer input, while native buttons keep their target size and `accessible name`s.
Internal-link fixtures check relative destinations, escaped Unicode titles, explicit aliases, literal code, recovery records, and stale revisions.
Graph controls follow native [button keyboard behavior](https://www.w3.org/WAI/ARIA/apg/patterns/button/) and keep fixed target sizes during zoom.
