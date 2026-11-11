# Storage evaluation

## Decision question

Choose storage for a personal note service that preserves substring search, conditional writes, recovery, workspace membership, and original files.
Prefer a Rust implementation when behavior, resource use, reliability, and maintenance costs meet the same requirements.
Compatibility with earlier unreleased storage formats is outside the evaluation.
Keep production SQLite while comparing complete tested behavior.

## Controls

The `sqlite` adapter uses the production store and its FTS5 indexes.
The `sqlite-kv`, `redb`, and `fjall` adapters share source records, packed Unicode postings, index differences, query verification, and lifecycle code.
The SQLite control distinguishes engine effects within the shared layout from the effect of replacing the app layout.
Shared-layout results apply to the selected layout and configuration.
Other layouts and settings can change the ranking.

The SQLite control has a 2 MiB writer page cache and a 64 KiB committed-view reader page cache.
The production store has a suggested 2 MiB cache.
The redb adapter has a 2 MiB cache, and Fjall has a 2 MiB cache with a 4 MiB memtable threshold.
The process and storage limits remain common across adapters.

Ordinary mutations wait for SQLite `synchronous=FULL`, redb `Durability::Immediate`, or Fjall `PersistMode::SyncAll` before their timing ends.
SQLite uses [write-ahead logging](https://sqlite.org/wal.html), whose [synchronization policy](https://sqlite.org/pragma.html#pragma_synchronous) governs commit durability.
The lockfile pins redb 4.5.0 and Fjall 3.1.13.
Process interruption tests check acknowledged and uncommitted operations.
They don't emulate loss of electrical power or establish hardware durability.

## Workloads

Use seeded repeated text, varied word combinations, alphanumeric text, populated collections, long-note edits, original files, ranges, cleanup, restoration, and process interruption.
Varied filler selects from 32 words.
Alphanumeric filler selects lowercase ASCII letters and digits and has more than 40000 distinct three-character windows in the tested 256 KiB sample.
Scaling uses private fixture loading with durable batches and a complete body audit.
Fixture loading isn't a measurement of individual app saves.
The operation phase adds 128 individual durable creations, repeated searches, and the selected update workload.
Original byte generation remains part of the upload input path for every adapter.

Four fresh-process repetitions rotate the order of the four adapters so each adapter occupies every position once.
Each repetition has a fresh anonymous data volume.
The runner doesn't control the host's operating-system cache.
Process high-water memory includes the runner, parsing, and the configured storage engine.
It isn't the production server's resident-memory measurement.

## Interpretation

Compare matching input recipes, source revisions, container limits, and execution environments.
Report medians and ranges across repetitions, with operation counts and within-run p50, p95, and p99.
Four repetitions describe observed variation without supplying a precise confidence interval or long-term failure rate.
Failed or incomplete experiments remain failures and can't become timing baselines.
Preparation and transaction timings identify boundaries within the shared implementation rather than explaining every microsecond of an app operation.

Adoption requires correct returned values and preserved source state before latency or storage gains can justify a change.
Compare note-heavy and attachment-heavy results together with measured memory, allocation, write traffic, and recovery behavior.
Untested hardware, workloads, prolonged operation, and missing workflows remain outside the findings.

## Initial comparison

The initial experiments ran on November 10, 2026, on an Apple M5 Pro host running aarch64 Linux through OrbStack.
Each experiment used two CPUs, 2 GiB of RAM, no swap, and a 900-second deadline.
The repeated experiments used a 4 GiB data allocation.
Other jobs ran on the host, and some long-edit repetitions had wide latency variation.
Keep the observed ranges rather than treating nearby medians as established winners.

All 224 experiments in the fourteen-scenario campaign completed across four adapters and four rotating repetitions.
The campaign used 128 initial 1 KiB notes, 64 updates where applicable, and 4 KiB originals, with 16 MiB originals in the large-file scenario.
Returned values, original bytes, conditional writes, cleanup, restoration, and interrupted writes passed the applicable checks.
The scenario names include overlapping operations and don't represent fourteen independent kinds of database behavior.

The 1000-note and 10000-note collections each completed sixteen experiments with varied 1 KiB notes, audited fixture loading, 128 later creations, and 100 edits to a 256 KiB note.
For 10000 initial notes, the median of the four save p95 values was 26.4 ms for production SQLite, 29.7 ms for redb, and 56.2 ms for Fjall.
Their respective run ranges were 25.6–28.1 ms, 25.7–33.4 ms, and 55.0–58.0 ms.
The mixed-search p95 medians were 11.7 ms, 1.3 ms, and 118.3 ms.
The 200 searches per run combined ten query forms, so the combined statistic isn't a per-query latency estimate.
Fjall's search p95 median was 1.7 ms at 1000 notes, showing scale-dependent latency in the measured configuration.

At 10000 notes, sampled peak data allocation was 84.1 MiB for production SQLite, 617.0 MiB for redb, and 553.7 MiB for Fjall.
Median process peaks were 9.8 MiB, 9.5 MiB, and 80.3 MiB.
Whole-workload writes, including fixture loading, were 482.7 MiB, 2681.2 MiB, and 1940.2 MiB.
The SQLite key-value control used 584.2 MiB of peak allocation and 6131.8 MiB of writes.
The shared record and index layout adds costs even when SQLite remains the engine.
Fjall's final file lengths were smaller than its sampled allocation peak because the peak includes transient storage during the workload.

All sixteen experiments with sixteen varied 1 MiB notes and 100 long-note edits completed.
Save p95 medians were 83.6 ms for SQLite, 82.5 ms for redb, and 80.2 ms for Fjall, with overlapping run ranges.
Sixteen equivalent repeated-text experiments also completed, but their broad latency ranges don't support a stable close ranking.
Packing grams and applying only changed postings removed avoidable work from the initial prototypes.

All sixteen alphanumeric experiments completed with one initial 1 MiB note and 40 later edits.
Save p95 medians were 971.2 ms for production SQLite, 332.7 ms for the SQLite key-value control, 363.3 ms for redb, and 363.7 ms for Fjall.
The respective run ranges were 835.8–1800.1 ms, 327.1–351.0 ms, 343.0–470.5 ms, and 344.6–453.9 ms.
The control demonstrates that the slower complete implementation in this case doesn't require replacing SQLite to improve it.
Shared-index preparation p95 medians were 318.5–348.1 ms, while transaction p95 medians were 14.4–17.3 ms.
Reducing full-body index preparation is a separate improvement opportunity from changing the engine.

All sixteen 64 MiB file experiments completed and verified three incompressible originals per run.
Median process peaks were 7.9 MiB for production SQLite, 7.0 MiB for redb, and 151.6 MiB for Fjall.
Median upload maxima across runs were 350.6 ms, 387.0 ms, and 174.3 ms.
Each run had three uploads, so its reported p95 is its largest observation.
The Fjall prototype trades a faster upload for much higher process memory in the selected atomic transaction layout.
Alternative staging and cleanup designs weren't measured.

The separate production `memory-profile` workload passed in its 64 MiB container.
The server peak was 13.4 MiB, below the 24 MiB gate.
The workload exercised 5000 notes, a near-limit note, and three 64 MiB file round trips.
The server measurement remains distinct from the storage runner's process peaks.

The 100000-note probe used an 8 GiB allocation and one experiment per adapter.
Only production SQLite completed the full workload, with a search p95 of 31.8 ms and a save p95 of 33.4 ms.
The SQLite key-value control reached the execution deadline without producing a complete report.
The redb and Fjall adapters loaded and audited all 100000 bodies but exceeded the 750 ms search deadline after completing 60 of the required 200 searches.
Their failing query was `needle-00099999`.
The failed runs aren't timing baselines, and the summary command rejected the incomplete comparison group.
These single runs describe feasibility within the selected limits rather than repeated latency estimates.

The initial prototype planner sampled at most 128 postings per candidate.
The generated identifier query has one full match, but its `999` posting has 379 notes and its `nee` posting has 100000 notes.
Both samples reached 128, and the tie rule chose the broader posting.
The ensuing verification scan reached the deadline.
This is a planner limitation within the measured shared layout, not evidence of an intrinsic 100000-note engine limit.
The follow-up changes replace sampled ranking with exact posting counts.

## Index improvement experiments

A fresh four-run SQLite baseline used one 1 MiB alphanumeric note, 40 edits, and a 128 MiB allocation.
Its save p95 median was 1135.7 ms, with run values from 855.9 to 1466.3 ms.
The measured input identity includes search requests.
Each following alphanumeric run passed compatibility checks with the fresh baseline's recipe, input identity, environment, and durable acknowledgements.

A native trigram trial with `detail=column` completed sixteen experiments across four adapters and four repetitions.
SQLite's save p95 median was 1127.2 ms, ranging from 932.2 to 2601.1 ms.
Median whole-run writes fell from 727.6 MiB to 129.3 MiB, but latency ranges didn't establish faster saves.
Omitting positions alone didn't resolve repeated tokenization work.

A second trial encoded distinct Unicode windows before inserting FTS5 tokens.
All sixteen alphanumeric experiments completed.
SQLite's save p95 median was 109.8 ms, with a 97.5–213.1 ms range.
Median sampled allocation peaks in fresh runs were 11.3 MiB, compared with 29.7 MiB in the fresh positional baseline.
The observation covers complete app saves rather than an isolated engine call.

The selectivity campaign added common positive terms, common negative terms, colliding windows, and a path combined with a negative term.
At 10000 notes, the trial SQLite adapter's first run had a common-negative p95 of 75.7 ms.
Its colliding-window p95 was 115.0 ms.
Presence-only windows need original-text verification even when the complete substring is absent.
All four Fjall repetitions exceeded the search deadline on the common-negative query.
The incomplete group remains a failed comparison rather than a timing baseline.

The production design retains positional rejection for normalized text of at most 16 KiB.
Larger text and text containing NUL use distinct Unicode windows and literal verification.
The size boundary preserves ordinary phrase rejection while limiting positional work for large bodies.
Body indexes include the filename and separating newline before classification.
Title indexes classify the full normalized title on their own.

Four production runs with the mixed index completed the same 1 MiB alphanumeric workload.
The save p95 median was 131.1 ms, with a 113.8–157.6 ms range.
The fresh positional baseline's corresponding median was 1135.7 ms.
The comparison supports faster saves for the tested large-body workload without requiring a different storage engine.

Four production selectivity runs completed with 100000 varied 1 KiB notes.
Their mixed-search p95 median was 64.0 ms, with a 62.2–82.9 ms range.
Common-negative, colliding-window, and path-filtered-negative p95 medians were 14.5 ms, 44.6 ms, and 14.7 ms.
Every measured search completed within the 750 ms deadline.
These results establish the tested limits rather than a universal search-latency bound.

The updated production `memory-profile` passed in its 64 MiB container.
Server peak memory was 13.6 MiB, below the 24 MiB gate.
The workload included 5000 notes, a near-limit note, and three 64 MiB original-file round trips.

Fresh databases create all source tables, workspace metadata, and derived search indexes in one transaction.
Regression tests compare literal results, short and long text, NUL, Unicode, pagination, title updates, and transitions across the size boundary.
The [unreleased-format decision](adr/initialize-only-the-current-storage-schema.md) removes automatic conversion and migration fixtures.
Unsupported format markers fail without rewriting saved records.
Current-format archive restoration rebuilds derived indexes from source records.
An exhausted note identifier also remains reachable through exact-title search after restoration.

The shared key-value adapters maintain posting counts within each durable index transaction and combine count changes within fixture batches.
The planner considers positive text and metadata conditions even when all text terms are negative.
Selective posting intersections reject nonmatching identifiers before metadata or body reads.
Postings that cover every note need no membership lookup, and a one-note candidate needs no further index probing.
Private fixture loading stops a batch after 500 notes or after reaching 4 MiB of input, matching production import.
The last note can cross the byte boundary.
Count tests cover batch visibility, edits, deletion, rollback, and malformed values.
Portable restoration rebuilds counts from source records.
Hash-based deduplication reduces repeated ordered-set operations during index preparation.

A counted-layout probe repeated the 100000-note, 1 KiB workload before the posting-intersection change.
Production SQLite completed, with a mixed-search p95 of 68.9 ms and a long-edit p95 of 23.0 ms.
The three shared-layout adapters reached the execution deadline without complete reports.
The reports establish an execution limit within the selected configuration, without supplying complete latency measurements for those adapters.
The failed comparison remains under `target/storage-profile-counted-scale`.

A diagnostic redb run used 100000 varied 128-byte notes after the posting-intersection change.
It loaded and audited every body and completed all 200 searches, including the identifier query that exposed sampled ranking.
Search p95 was 975 microseconds.
The largest search took 3734 microseconds.
Fixture loading took 300.9 seconds, sampled allocation peaked at 2.93 GiB, and process memory peaked at 7.7 MiB.
The single run establishes feasibility for that short-note recipe, without replacing the failed 1 KiB comparison or estimating repeated latency variation.
Its report remains under `target/storage-profile-counted-short-notes`.

A lifecycle regression campaign completed all sixty experiments across fifteen scenarios and four adapters after the posting-intersection change.
It used 64 initial 1 KiB notes, sixteen updates where applicable, and 4 KiB originals, with 16 MiB originals in the large-file scenario.
Applicable returned-value, original-byte, conditional-write, cleanup, restoration, and interrupted-write checks passed.
The campaign provides functional regression coverage with one run per adapter and scenario.
Reports remain under `target/storage-profile-lifecycle-regression`.

The [large-text index decision](adr/omit-term-positions-from-substring-candidate-indexes.md) and [posting-count decision](adr/maintain-posting-counts-with-durable-index-changes.md) record the tradeoffs and reconsideration conditions.
Raw reports remain in `target/storage-profile-position-baseline`, `target/storage-profile-compact-alphanumeric`, `target/storage-profile-deduplicated-alphanumeric`, and `target/storage-profile-index-selectivity`.
The mixed-index reports remain in `target/storage-profile-large-note-index` and `target/storage-profile-positional-selectivity`.

## Decision

Keep production SQLite as the app baseline.
Its checked app behavior and compact allocation remain useful for ordinary note collections and large originals.
Use distinct windows for large text while preserving positional rejection for ordinary text.

The redb adapter is the Rust candidate to investigate first because it combines low process memory, fast search at 1000–10000 notes, and long-body editing.
Its small-note collections have higher index allocation and creation costs in the initial comparison.
The counted planner addresses the sampled-ranking defect, while broad verification scans remain a layout constraint.
Fjall offers faster 64 MiB uploads, but its measured search scaling and file-transaction memory require changes before adoption under the app's low process-memory goals.
The experiments don't establish complete app compatibility or long-term reliability.

The comparison doesn't rank other embedded engines, a separate Tantivy index, other cache settings, or compressed posting-list layouts.
The seeded inputs aren't a representative sample of users' notebooks.
Power-loss durability, concurrent service traffic, and prolonged operation remain outside the measurements.

## Reproduction

Run `mise x -- just storage-profile --engine all --case all --count 128 --updates 64 --file-bytes 4096 --repeats 4 --allocation-gib 4` for the expanded scenario campaign.
The current `all` selection includes fifteen cases, while the initial comparison covered fourteen.
Run `mise x -- just storage-profile --engine all --case long-edits --count 10000 --updates 100 --content varied --load-fixture --repeats 4 --allocation-gib 4` for the populated collection.
Run `mise x -- just storage-profile --engine all --case long-edits --count 1 --note-bytes 1048576 --updates 40 --content alphanumeric --repeats 4 --allocation-gib 4` for the high-cardinality text.
Run `mise x -- just storage-profile --engine all --case incompressible-files --count 16 --file-bytes 67108864 --repeats 4 --allocation-gib 4` for 64 MiB originals.
Run `mise x -- just storage-profile --engine sqlite --case search-selectivity --count 100000 --content varied --load-fixture --repeats 4 --allocation-gib 2` for production search selectivity.
Use `--engine redb --case short-notes --count 100000 --note-bytes 128 --content varied --load-fixture` for the short-note diagnostic.
Use `--engine sqlite --allocation-mib 128` with the alphanumeric recipe to repeat the production large-note comparison.
Inspect each plan with `--dry-run` before allocating resources.
Summarize each complete campaign with `mise x -- just storage-summary --input <directory> --repeats 4`.
Add `--engine sqlite` for a production-only campaign.
Local requests, native reports, diagnostics, and summaries remain under `target/storage-profile-*` and `target/storage-*-summary.log`.
