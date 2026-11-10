# Security boundaries

## Intended deployment

A single owner runs the app on trusted machines inside a private tailnet.
The design trusts the operating system, browser profile, server process, proxy, and software supply chain.
The app has no accounts or content encryption keys.
Every device allowed to reach the listener can read, edit, export, and delete notes.
Use the tailnet policy to restrict access to the owner's devices.
Don't expose the listener to the public Internet.

## Stored content

The SQLite database, journals, exports, backups, and browser drafts contain readable content.
Disk encryption protects locked storage from offline reading.
For example, [FileVault](https://support.apple.com/guide/deployment/intro-to-filevault-dep82064ec40/1/web/1.0) protects data on a Mac's startup disk.
Protect backup destinations independently.
An unlocked account, running app, copied export, or compromised administrator remains outside that protection.
Deleting a note or expiring a file doesn't erase external backups.
Storage devices may keep deleted blocks.

The source archive preserves the encrypted prototype.
The prototype protected storage copies while trusting the running server with decryption keys.
The note process could read unlocked content.
The current design delegates storage encryption to the operating system and backup tools.

## App protections

Writes require same-origin JSON or the defined binary attachment transport.
The HTTP boundary rejects unrecognized hostnames and cross-origin browser writes.
An optional configured HTTPS origin requires the exact Host and Origin.
Keep loopback publishing and HTTPS proxying as separate deployment controls.
The app has no cross-origin resource sharing policy that grants another website access.

Markdown rendering rejects active HTML and unsafe destinations.
A restrictive Content Security Policy, validated media types, and content-type sniffing protection constrain browser content.
Original attachments remain unmodified.
HTML and other active file types download instead of becoming app documents.

Content revisions reject stale writes.
A single process lock, serialized mutations, and SQLite transactions preserve related bodies and indexes together.
Browser draft ownership and refresh guards prevent remote replies from replacing local writing.
Identifiers, request sizes, pagination, query execution, media ranges, and maintenance batches have explicit bounds.
Schema, directory, symbolic-link, and hard-link checks reject unsupported storage rather than constructing an empty replacement.

## Remaining risks

A vulnerability executing code inside the note process can read or alter all notes.
[Docker isolation](https://docs.docker.com/engine/security/) limits the process's host access.
Isolation doesn't protect notes handled by that process.
The provided deployment uses an unprivileged user, a read-only root filesystem, dropped capabilities, and bounded resources.
Host administrators and compromised kernels remain trusted.
Disk encryption doesn't prevent these runtime attacks.

Browser extensions or malicious app code can read visible notes and unsaved drafts.
A shared browser profile shares that access.
Restrict software installation and keep trusted components updated.
The dependency and secret gates check known advisories and committed content.
Passing tests or bounded proofs don't establish the absence of exploitable vulnerabilities.
