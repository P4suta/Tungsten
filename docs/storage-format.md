# Portable storage archives

Stop the service before running `tungsten --data-dir ./notes --backup-to ./backup`.
Restore with `tungsten --data-dir ./restored --restore-from ./backup`.
Choose an unused destination outside its source directory.
The commands validate and synchronize staged content before publishing their destination.
Successful completion also synchronizes the destination's parent directory.
The existing `--export-to` command remains a smaller Markdown export.

An archive contains `manifest.json`, `metadata.jsonl`, `notes`, `recovery`, and `files` when those payloads exist.
Markdown retains its original UTF-8 bytes.
Files keep their original binary bytes and attachment identifiers.
The archive includes unreferenced originals that remain within retention, historical notes, Trash, pins, workspaces, memberships, and pending maintenance.
Browser drafts remain browser-held state.
Configure backup encryption outside the app.

## Manifest and metadata

The manifest declares format `1`, the metadata record count, and `metadata_revision`.
Revisions use the existing quoted SHA-256 representation.
The metadata revision covers the exact JSON Lines bytes, including each terminating newline.
Missing records, extra records, unsupported formats, and malformed lines fail restoration.
Unknown fields in the manifest, record envelopes, and typed cells fail restoration.
The existing attachment parser accepts unknown fields inside original attachment metadata.
Each metadata line contains a `kind` and a `values` array.
Each cell uses a `text`, `integer`, `bytes`, or `null` type tag.
The first three types carry a `value`.
Null cells carry only their type tag.
Byte cells use JSON arrays of unsigned bytes.
Integer cells use signed 64-bit JSON numbers and require a lossless integer parser.
Metadata lines have a 16 MiB limit, and the manifest has a 1 KiB limit.
The metadata limit accommodates two near-limit titles with JSON escaping in a pending title update.

A document record contains its name, body revision, modification time, and stable index identity.
Its body lives at `notes/<identity>.md`, using the stable index identity as 16 lowercase hexadecimal digits.
Identity paths preserve names that collide under filesystem case folding or Unicode normalization.
The metadata keeps the original note name, including its directory components.
A snapshot record contains its Trash flag, snapshot identity, note name, body revision, modification time, capture time, and recovery sequence.
Its body lives at `recovery/<flag>/<identity>.md`.
A file record contains its identity, original metadata bytes, leading bytes, complete revision, binary revision, modification time, and byte count.
Its original lives at `files/<identity>`.
The complete file revision hashes binary bytes followed by original metadata bytes.
The binary revision hashes only the original binary bytes.

Other record kinds preserve their ordered source columns.
Pin and retention records contain a note name, and dirty-file records contain a file identity.
File-clock records contain the file identity, revision stamp, and nullable time of becoming unused.
Title-update records contain the job identity, target name, previous title, current title, target revision, processed note identity, and ready flag.
State records contain a key and integer value for note count, recovery sequence, Trash purge position, or completed import state.
Workspace records contain identity, title, and revision counter.
Membership records contain sequence, workspace identity, and note name.
Workspace-state records contain the fixed identity `1` and collection revision counter.
Sequence records preserve the allocated identity counters for notes, title updates, workspaces, and memberships.
Times are nonnegative milliseconds since the Unix epoch.

## Restoration

The reader validates names, identities, revisions, cell types, payload limits, references, counters, and payload hashes.
It rejects symbolic links and multiply linked payload files.
Checksums detect changed content.
The format doesn't authenticate archive provenance.
Only restore archives from a trusted source.

SQLite restoration rebuilds search, links, attachment references, recovery summaries, and graph degrees from original content.
It restores source counters and maintenance clocks after index construction.
Workspace and membership identities, ordering, and revision counters survive the round trip.
Capture times and cleanup clocks keep their original deadlines.
Restoring an older archive doesn't restart retention periods.
The implementation processes one stored note at a time and streams binary payloads through bounded buffers.
Failures before publication leave the requested destination unpublished.
A parent-directory synchronization failure can leave a complete destination published while the command reports an error.
