# Run the service

Build with the commands in the [README](../README.md).
The executable embeds browser assets and SQLite.
Node.js and a separate database server are unnecessary at runtime.
Start with `./target/release/tungsten --data-dir ./notes` and open [localhost](http://localhost:8080).
The default listener is `127.0.0.1:8080`.
Opening the app immediately shows the last note or a page ready for writing.

## Private access

The app has no accounts, unlock screen, recovery keys, or device registration.
Use the operating system and the tailnet policy to restrict access.
Anyone with network access to the service can read and change notes.
Keep the listener on loopback and expose it through a private HTTPS proxy.
[Tailscale Serve](https://tailscale.com/docs/features/tailscale-serve) can publish a local service within the tailnet.
Use `--origin https://notes.example` to require an exact Host and Origin behind a proxy.
The proxy must preserve the configured Host.
Omit request bodies and private query strings from proxy logs.
Read the [security boundaries](security.md) before exposing the service.

## Container

Create a private `notes` directory for the container's numeric user.
Set `TUNGSTEN_UID` and `TUNGSTEN_GID` when the account differs from 1000.
Run `docker compose up --build -d`.
Compose binds the host port to loopback and mounts the data directory.
The process runs without root privileges, a writable root filesystem, or added capabilities.
The temporary filesystem, CPU, memory, swap, and process counts have explicit limits.
Container isolation doesn't hide notes from the note process.

## Memory use

The server keeps one SQLite connection with a suggested 2 MiB page cache and memory mapping turned off.
Saving a note extracts index metadata without rendering HTML or constructing preview offsets.
Note pages, graph neighborhoods, recovery lists, and background jobs use bounded batches.

Binary uploads stream through an unnamed file beside the database and 64 KiB database buffers.
The OS removes the staging file when its final handle closes, including interrupted uploads.
Staging needs up to 64 MiB of disk space while the existing upload slot remains occupied.
JSON attachment requests remain available for small clients with a 1 MiB request limit.
The browser sends original binary files through the full 64 MiB attachment path.

SQLite temporary storage can spill to files instead of forcing temporary tables into memory.
The container and systemd examples set `SQLITE_TMPDIR` to the data directory to avoid a memory-backed temporary filesystem.
Override `SQLITE_TMPDIR` when changing the container's data path.
Reading and navigating saved notes doesn't load the Markdown formatter.
Formatting loads after editing or an explicit keyboard command.
Run the bounded [memory checks](verification.md#memory-footprint) to measure the production executable.

## Service management

Install the executable as `/usr/local/bin/tungsten` and use [the systemd unit](../deploy/tungsten.service).
The unit owns `/var/lib/tungsten` through a dynamic service account.
The unit listens on loopback without requiring an environment file.
Configure the origin in a unit override when using a canonical proxy hostname.

## Storage and recovery

Keep one service process per data directory on local Unix storage.
The directory contains `notes.sqlite`, SQLite journal files when present, and the process lock.
The database holds Markdown bodies, indexes, links, original attachments, history, Trash, workspaces, pins, and maintenance queues.
New databases receive the complete current schema in one transaction, including workspace membership.
Search indexes use distinct Unicode windows for normalized text larger than 16 KiB and text containing NUL.
Ordinary text retains positional trigram indexes.
Unreleased storage formats have no compatibility support or automatic conversion.
Unsupported databases require recreation.
It contains readable content.
Configure disk and backup encryption outside the app.

Stop the service before copying or restoring the complete data directory.
Copying a running database requires a consistent copy of its write-ahead log.
SQLite's [journal documentation](https://www.sqlite.org/wal.html) describes the journal files and the need for local storage.
Unknown directories, unsupported schemas, and incomplete databases fail explicitly.
The app doesn't scan or import external Markdown files.
Export Markdown and original files before using another editor.
Stop the service and run `tungsten --data-dir ./notes --export-to ./exported-notes` to write the current notes and referenced originals.
Export preserves source text and identifiers, excludes Trash and history, and refuses an existing destination.
It reads attachment chunks and publishes the directory only after completion.
Use a complete database backup to keep recovery records.
Use `tungsten --data-dir ./notes --backup-to ./backup` for a complete portable archive.
Stop the service before restoring with `tungsten --data-dir ./restored --restore-from ./backup`.
Choose an unused destination outside the source directory.
Read the [archive format](storage-format.md) for retained metadata, validation, and retention behavior.
The encrypted prototype requires its archived executable and source.
The current app doesn't convert its data.

Recovery checkpoints group continuous edits and preserve structural or idle boundaries.
History keeps at most 50 visible versions per note.
History and Trash expire after seven days through bounded maintenance.
Files remain while current notes or recovery records reference them.
After the final reference disappears, files receive a further seven-day grace period.
Explicit file deletion in an embedded viewer removes original bytes after confirmation.
External backups keep independent copies.

The browser keeps unsaved drafts in local storage.
Tabs keep separate draft ownership.
Clean open notes refresh on focus and every fifteen seconds while visible.
Dirty notes preserve writing and report conflicting revisions.
Saving requires the exact acknowledged revision.
Another browser can open the same URL without registration.

## Sample notebooks and capacity

Run `mise x -- just demo-notes` to generate a separate synthetic database under `target/demo-notes`.
The default seed creates 10000 pages across 20 subjects and 400 topic maps.
The command refuses an existing destination.
Use `--output`, `--count`, and `--seed` for repeatable collections.

Serve a sample with `target/release/tungsten --data-dir target/demo-notes --listen 127.0.0.1:8081`.
The default [Philosophy entry](http://localhost:8081/?note=000000000000002a0000000000000000.md) links into the sample.
Use `mise x -- npm --prefix ui run test:demo` with the running sample.

Inspect `mise x -- just scale-profile --dry-run` before starting a capacity campaign.
The maintained runner uses two CPUs, 2 GiB of RAM, no swap, no network, and a 900-second deadline.
A campaign generates at most 1200000 notes in ordinary SQLite and audits stored Markdown destinations.
The mixed profile includes duplicate titles, disconnected notes, Unicode, long notes, and intentional unwritten destinations.
Requests must stay within 4 MiB, with a ninety-fifth percentile of at most 500 milliseconds and no response exceeding one second.
Eight clients exercise mixed requests concurrently.
Use `--counts 1000000 --extremes` to test a million incoming links and a million recovery records.
The default allocation budget is 8 GiB.
The `--allocation-gib` argument accepts up to 12 GiB after agreeing the machine's budget.
The runner prints the retained Docker volume.
Remove only an unused campaign's named volume.

## Storage comparisons

Run `mise x -- just storage-profile --dry-run --engine all --case regression` before comparing engines.
Run the same command without `--dry-run` to compare SQLite, redb, and Fjall in separate Linux containers.
The `sqlite-kv` control uses the same app and index code as redb and Fjall with a SQLite key-value table.
The `all` selection includes the production SQLite adapter and all three shared-index adapters.
The app continues to use SQLite.
The other engines are development prototypes with transactional source records, substring indexes, incoming links, recovery checkpoints, and chunked originals.
Results cover the tested operations rather than establishing complete compatibility with every app workflow.

Select `short-notes`, `long-edits`, `burst`, `hub`, `unicode`, `small-files`, `large-files`, `incompressible-files`, `shared-files`, `churn`, `recovery`, `crash`, or `capacity` through `--case`.
Use `search-selectivity` for measurements of common terms, negative terms, and colliding search windows.
Use `--case all` to run every case sequentially for each selected engine.
Use `--count`, `--note-bytes`, `--file-bytes`, `--updates`, and `--seed` to change the workload.
Use `--content varied` for seeded word combinations instead of repeated filler.
Use `--content alphanumeric` for seeded lowercase letters and digits with many distinct search grams.
Use `--repeats 4` for four fresh datasets with rotating engine order.
Use `--load-fixture` to prepare large collections in durable batches and audit every original body before measuring operations.
Fixture mode reports loading and auditing as setup phases and measures 128 individual durable creations after the initial search checks.
Long-edit cases use at least 256 KiB, and large-file cases use at least 16 MiB.
The existing 1 MiB note and 64 MiB file limits remain in effect.
The capacity case checks transaction aborts and short transfers.
Allocation limits reject oversize plans and runs.

Each container has two CPUs, 2 GiB of memory, no swap, 256 processes, no network, and a 900-second safety deadline.
The default storage allocation is 8 GiB, and `--allocation-gib` accepts a smaller limit.
For small fixtures, use `--allocation-mib` with 16 to 8192 MiB instead of `--allocation-gib`.
The free-space reserve remains 1 GiB with either unit.
The runner rejects plans over the selected allocation and samples data allocation every 250 milliseconds during the workload.
An observed allocation over the selected limit terminates the run.
Each experiment requires free space for its allocation and a further 1 GiB reserve before loading data.
Sampling can miss a transient allocation peak between observations.
The default campaign budget is 3600 seconds after building the image.
The `--max-seconds` argument accepts 900 to 7200 seconds within the agreed resource window.
The runner stops admitting experiments when the remaining budget can't contain the 900-second experiment deadline.
Docker removes anonymous data volumes when their containers exit.
The runner retains requests, measurements, and failure diagnostics under a fresh `target/storage-profile-*` directory or an unused `--output` directory.
Use `--baseline <measurement.json>` to compare matching workloads on the same Docker daemon and execution environment.
Changing the seed, generated inputs, limits, or environment refuses the comparison.
Search requests contribute to the input identity.
Changes to input-signature construction require a fresh baseline, while historical timing reports remain readable.
Incomplete measurements never become baselines.

Format 2 measurements include operation counts, input byte counts, `p50_us`, `p95_us`, `p99_us`, and `max_us` latency, process memory, process I/O, storage allocation, reopening, and restoration.
Shared-index adapters report preparation and transaction durations for long-note updates and the number of changed index entries.
Search planning uses exact posting counts updated within each durable index transaction.
The `allocated_bytes` field reports the largest observed allocation, and `logical_bytes` reports final file lengths.
Every successful mutation is durable before its timing ends.
Read, search, range, conflict, and recovery checks validate returned values.
Process reopening starts a fresh store without claiming a cold operating-system cache.
Timing changes are observations for investigation.
The existing production performance and memory gates keep their own workloads and budgets.
Run `mise x -- just storage-summary --input <campaign-directory> --repeats 4` to summarize four complete repetitions for every engine and workload present.
Use `--engine sqlite` to summarize a production-only campaign with the same completeness checks.
The summary rejects failed runs, missing repetitions, duplicate repetitions, changed source, mismatched generated inputs, and incompatible environments.
It reports the median and range of each run's p95 alongside process and storage observations.
Read the [evaluation method](storage-evaluation.md) before interpreting engine rankings.
