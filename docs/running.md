# Run the service

Build with the commands in the [README](../README.md).
The executable includes the browser assets and SQLite and needs no separate database server or Node.js runtime.
The default data directory contains an encrypted SQLCipher database and a public vault descriptor.
Keep one service process per data directory on a local Unix filesystem.
Each note can contain up to 1 MiB of text.
Note names are visible relative paths ending in `.md`.
A path supports at most 240 bytes across 16 components.
The service rejects hidden components, parent components, empty components, backslashes, and ASCII control characters.
The [feature guide](features.md) explains editing, organization, attachments, and recovery.

## Private access

Run the executable with `--data-dir ./notes --origin https://notes.example` and the default localhost listener.
Replace the example origin with the dedicated HTTPS hostname that serves the app.
Use `tailscale serve --bg 8080` to expose the service inside the tailnet over HTTPS.
The [Tailscale setup guide](https://tailscale.com/docs/solutions/host-private-server-docker) explains the certificate prerequisite.
The existing tailnet policy determines access.
The app has no user accounts.
The first browser generates a vault key and an independent recovery key.
Save the recovery key in a password manager before starting the vault.
Browser enrollment remembers a non-extractable key in IndexedDB.
New browsers need the recovery key.
After a server restart, an enrolled browser supplies a derived runtime key automatically.
The server writes no decryption key to its data directory.
Every private request needs a process-lifetime device session, including original file requests after another browser unlocks the server.
The HTTP boundary checks the configured Host and Origin, and rejects browser writes from another origin.
Reserve the entire HTTPS hostname for the app because [cookies share ports](https://www.rfc-editor.org/rfc/rfc6265.html#section-8.5).
Use the canonical lowercase origin without an explicit default port, path, query, or fragment.
The trusted TLS proxy must preserve the configured Host.
Proxy logs must omit private query strings, request bodies, and device cookies.
Restrict first enrollment to the owner's device through the tailnet policy.

For local testing, use `--development` with a loopback listener.
Development mode uses HTTP and requires a trusted local machine and browser profile.
The server refuses development mode on a non-loopback listener.
Use `--plaintext` only for legacy installations and synthetic test collections.

The selected threat model protects offline copies of app storage and backups, and rejects clients without a device session.
Bodies, search indexes, recovery records, file metadata, and original bytes use [SQLCipher page encryption and authentication](https://www.zetetic.net/sqlcipher/design/).
The running server, browser-code delivery, TLS proxy, operating systems, and enrolled browser profiles remain trusted.
A runtime compromise can read unlocked notes and keys.
A malicious browser-code distributor can steal access when a browser executes its code.
Runtime compromise and code-distribution compromise grant different capabilities.
The design makes no claim to conceal live search patterns from the running server or to prevent operating systems from persisting RAM through swap or crash dumps.
Temporary database stores use memory.
The client keeps no corpus-sized search index and performs no idle index synchronization.
The browser keeps plaintext unsaved drafts inside the trusted browser profile.
Losing every enrolled browser and the recovery key makes the vault unreadable.

## Migrate a plaintext installation

Keep the running installation unchanged while preparing the new executable.
Stop the source service before copying its data.
Run the new executable with a separate empty `--data-dir`, `--migrate-from` pointing to the source directory, and the chosen origin.
Enroll the browser and save its recovery key.
The server locks the source, copies its catalog and payloads in the background, and keeps private routes locked until verification succeeds.
The copy preserves note identities, timestamps, links, pins, history, Trash visibility, and original file metadata and bytes.
An interrupted copy requires the same source directory and recovery key when restarting with `--migrate-from`.
A changed source stops the copy instead of publishing inconsistent notes.
Source notes, recovery records, and original files remain available.
Opening a legacy source can update its catalog and migrate its earlier Trash layout.
Verify the encrypted copy before changing the service's data directory and removing plaintext sources.
External backups, Git repositories, filesystem snapshots, and storage-device remnants keep their own copies.

## Container

Create a `notes` directory owned by the account that runs the container.
Set `TUNGSTEN_UID` and `TUNGSTEN_GID` to that account's numeric identifiers when they differ from 1000.
Set `TUNGSTEN_ORIGIN` to the dedicated HTTPS origin.
Start with `docker compose up --build -d`.
Compose binds the host port to localhost and mounts the notes directory into the container.
The image runs without root privileges and needs no writable root filesystem.
Compose provides a bounded temporary filesystem for database operations.

## Service management

Install the executable as `/usr/local/bin/tungsten` and use [the service unit](../deploy/tungsten.service) for systemd.
The unit owns `/var/lib/tungsten` through a dynamic service account.
Set `TUNGSTEN_ORIGIN` in `/etc/tungsten.env` before starting the unit.
Stop the unit before restoring its data directory.

## Sample notebook

Run `mise x -- just demo-notes` to generate a separate notebook under `target/demo-notes`.
The default seed creates 10000 pages across 20 subjects and 400 topic maps, with 57996 internal links.
The text is synthetic material for exploring the editor and graph.
The command refuses an existing destination and publishes completed output without replacing another path.
Use `--output`, `--count`, and `--seed` to choose another destination, 420 to 10000 pages, and a repeatable seed.

Run `target/release/tungsten --plaintext --data-dir target/demo-notes --listen 127.0.0.1:8081` to serve the sample notebook.
Open the [Philosophy entry](http://localhost:8081/?note=000000000000002a0000000000000000.md) for the default seed and follow its links or open Explore notes.
Run `mise x -- npm --prefix ui run test:demo` with the running sample service to exercise the full collection in desktop and mobile browsers.
The optional `TUNGSTEN_DEMO_URL` environment variable selects another sample service.
Set `TUNGSTEN_DEMO_COUNT` when testing a collection with a different page count.

## Scale profiling

Run `mise x -- just scale-profile --dry-run` to inspect the workload before generating a larger collection.
Run `mise x -- just scale-profile` to generate and inspect 10000, 100000, and 1000000 real Markdown files in a fresh Docker volume.
The command uses OrbStack on macOS.
The default `mixed` profile includes isolated notes, disconnected subjects, terminal notes, a popular hub, duplicate titles, missing headings, Unicode, longer entries, and intentional links to unwritten notes.
Fenced Markdown examples contain literal links that the audit must ignore.
The `clustered` profile keeps every destination present and uses the same subject structure as the sample notebook.
Use `--seed`, `--profile`, and `--counts` to select repeatable variants.
Use `--encrypted --counts 1000000` to build original note payloads and indexes directly inside SQLCipher and exercise the device-session HTTP boundary.
The encrypted fixture uses public synthetic keys and checks allocated database blocks during generation and before publication, with a default budget of 8 GiB.
The optional `--allocation-gib` argument selects a limit from 8 to 12 GiB after agreeing the machine's storage budget.
An incomplete encrypted fixture can resume with `--reuse-volume` after checking its stored sample bodies.
The fixture requires no plaintext source files and leaves the browser-enrollment cryptography to the browser suite.
One campaign accepts at most 1200000 pages in increasing dataset sizes.

The runtime has two CPUs, 2 GiB of memory, no swap, 256 processes, no network, and a time limit of 900 seconds.
Generation writes one note at a time and limits allocated source file blocks to 8 GiB.
SQLite indexes need extra disk space.
The profiler reads every generated file and checks actual Markdown destinations before starting the service.
It then measures startup, opening a note, searching, paginated listings, and bounded graph neighborhoods.
Requests use a browser deadline of 15 seconds and a response budget of 4 MiB.
Every interactive endpoint runs 20 times and must keep its ninety-fifth percentile within 500 milliseconds and every response within one second.
Eight concurrent clients exercise mixed search and graph requests through the same latency gate.
The dataset stays in the named volume printed by the command.
Remove an unused dataset with `docker volume rm` and its printed volume name.

Failed startup, HTTP errors, oversize responses, timeouts, and exceeded latency limits fail the command.
A stopped generation or incomplete audit leaves the capacity result inconclusive.
Passing results cover the generated workload and measured environment.

Use `--reuse-volume` with a printed volume name to reuse an audited dataset and measure a warm index.
Use `--counts 1000000 --extremes` to create a separate collection with 999999 links into one note and 1000000 recovery records for that note.
The extreme fixture limits allocated source blocks to 12 GiB and shares immutable record bytes across 4096 variants while retaining a separate directory and metadata entry for every record.
Checks read actual recovery sources, save the hub, preserve explicit aliases, restore deleted text, and perform bounded cleanup.
Initial index construction has a separate startup budget of 600 seconds within the campaign deadline.

## Recovery

Stop the service before copying or restoring the complete data directory with ordinary filesystem tools.
An encrypted backup needs both `.vault.json` and `.sealed.sqlite`.
Copying a running database also requires a consistent copy of its write-ahead log.
The app refuses incomplete backups instead of creating an empty replacement.
Recovery points, Trash, pins, files, and indexes share the encrypted database.
Bounded maintenance reclaims deleted database pages.
Legacy plaintext installations use the following directory layout.
Delete moves notes into records under `.trash`.
Open More and Trash to restore notes within seven days through the dedicated Trash page.
Each record contains `body.md` and `metadata.json`.
The service migrates trash files from the earlier flat layout when opening the directory.
Version history, pins, and attachments use `.history`, `.pins`, and `.attachments`.
Search, link indexes, durable work queues, and cleanup clocks use `.index`.
Keep the complete data directory in backups to preserve every record.
The app expires recovery points and trash after seven days.
Unreferenced attachments have a further seven-day grace period before bounded background cleanup deletes them.
Permanent file deletion from an embedded viewer removes original bytes immediately after confirmation.
Backups and external Git repositories keep their own copies independently.
Opening a plaintext note reads its source file.
Search and graph navigation use the local catalog.
Restart after external edits, so the catalog can reconcile changed files.
The service has no file watcher.
Concurrent external editing is outside the save protocol.

The browser keeps unsaved drafts in local storage.
Tabs keep separate draft ownership and leave a living sibling's writing available for explicit recovery.
The open note checks for remote changes on foreground transitions and every fifteen seconds while visible.
Successful writes notify sibling tabs within the same browser origin.
Clean notes adopt remote changes while local edits remain recoverable after a conflict.
Retry a failed save, or save a separate copy when another tab has changed the saved revision.
Reload saved discards a draft after confirmation.
If browser storage is unavailable, an error asks you to keep the tab open until the server acknowledges the save.
