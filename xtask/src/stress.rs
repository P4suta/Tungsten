use crate::demo::Corpus;
use anyhow::{Context, Result, ensure};
use rusqlite::{OptionalExtension, params};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tungsten::{
    domain::{NoteName, Revision},
    store::{Note, SavedNote, Store},
};

pub(super) fn prepare(
    directory: &Path,
    corpus: &Corpus,
    deadline: Instant,
    allocation_gib: u64,
) -> Result<()> {
    fs::create_dir_all(directory)?;
    let mut store = Store::open(directory)?;
    let imported = store.search("", None, 1, false)?.total;
    ensure!(
        imported <= corpus.count,
        "stress fixture has unexpected notes"
    );
    let hub = corpus.name(0);
    store.import_notes((imported..corpus.count).map(|index| {
        if index.is_multiple_of(10_000) && Instant::now() >= deadline {
            return Err(tungsten::Error::Limit(
                "Stress generation deadline exceeded.",
            ));
        }
        let body = if index == 0 {
            corpus.note(0).body
        } else {
            format!("# Stress note {index}\n\nUnicode 日本語.\n\n[Scale hub]({hub})\n")
        };
        Ok(Note {
            name: NoteName::parse(corpus.name(index))?,
            revision: Revision::of(body.as_bytes()),
            body,
            modified: 1_735_689_600_000 + index as u128,
        })
    }))?;
    drop(store);
    let mut database = rusqlite::Connection::open(directory.join("notes.sqlite"))?;
    database.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")?;
    let captured = database
        .query_row(
            "SELECT value FROM state WHERE key='stress-captured'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(i64::try_from(
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
        )?);
    database.execute(
        "INSERT OR IGNORE INTO state VALUES('stress-captured',?)",
        [captured],
    )?;
    let count: i64 =
        database.query_row("SELECT count(*) FROM records WHERE trash=0", [], |row| {
            row.get(0)
        })?;
    for start in (usize::try_from(count)?..corpus.count).step_by(500) {
        ensure!(
            Instant::now() < deadline,
            "stress recovery generation deadline exceeded"
        );
        let transaction = database.transaction()?;
        for index in start..(start + 500).min(corpus.count) {
            let id = format!("{index:032x}");
            let body = format!("# Snapshot {index}\n\nSynthetic recovery record 日本語.\n");
            let revision = Revision::of(body.as_bytes());
            let timestamp = captured.saturating_sub(index as i64);
            let summary = SavedNote {
                id: id.clone(),
                name: NoteName::parse(hub.clone())?,
                modified: captured as u128,
                revision: revision.clone(),
                bytes: body.len(),
                captured: timestamp as u128,
                title: format!("Snapshot {index}"),
            };
            transaction.execute(
                "INSERT INTO snapshots VALUES(0,?,?,?,?,?,?)",
                params![id, hub, body, revision.as_str(), captured, timestamp],
            )?;
            transaction.execute(
                "INSERT INTO records VALUES(0,?,?,?,?,?,?)",
                params![
                    id,
                    hub,
                    timestamp,
                    serde_json::to_string(&summary)?,
                    revision.as_str(),
                    (index + 1) as i64
                ],
            )?;
        }
        transaction.execute(
            "UPDATE state SET value=? WHERE key='record-sequence'",
            [(start + 500).min(corpus.count) as i64],
        )?;
        transaction.execute("INSERT OR IGNORE INTO retention_queue VALUES(?)", [&hub])?;
        transaction.commit()?;
        if start.is_multiple_of(10_000) {
            use std::os::unix::fs::MetadataExt;
            let allocated = fs::read_dir(directory)?.try_fold(0_u64, |total, entry| {
                Ok::<_, std::io::Error>(total + entry?.metadata()?.blocks() * 512)
            })?;
            ensure!(
                allocated <= allocation_gib * 1024 * 1024 * 1024,
                "stress allocation exceeded its budget"
            );
            eprintln!(
                "operation=stress phase=generate records={start} total={} allocated_bytes={allocated}",
                corpus.count
            );
        }
    }
    let count: i64 =
        database.query_row("SELECT count(*) FROM records WHERE trash=0", [], |row| {
            row.get(0)
        })?;
    ensure!(
        usize::try_from(count)? == corpus.count,
        "unexpected stress recovery count"
    );
    println!(
        "operation=stress phase=data_verified notes={} hub_incoming={} recovery_records={count}",
        corpus.count,
        corpus.count - 1
    );
    Ok(())
}

pub(super) fn storage_checks(directory: &Path, hub: &str, deadline: Instant) -> Result<()> {
    let started = Instant::now();
    let mut store = Store::open(directory)?;
    println!(
        "operation=stress phase=reopen elapsed_ms={}",
        started.elapsed().as_millis()
    );
    let hub = NoteName::parse(hub.to_owned())?;
    let started = Instant::now();
    let history = store.history(&hub)?;
    ensure!(
        history.len() == 50,
        "history must return the newest 50 records"
    );
    for pair in history.windows(2) {
        ensure!(
            pair[0].captured >= pair[1].captured,
            "history order changed"
        );
    }
    for record in &history {
        ensure!(
            store.saved_note(&record.id, false)?.revision == record.revision,
            "history source validation failed"
        );
    }
    println!(
        "operation=stress phase=history records={} elapsed_ms={}",
        history.len(),
        started.elapsed().as_millis()
    );
    ensure!(
        started.elapsed() < Duration::from_secs(1),
        "history exceeded one second"
    );
    let original = store.read(&hub)?;
    let started = Instant::now();
    let changed = store.update(
        &hub,
        &original.body.replacen("# Philosophy", "# Updated hub", 1),
        original.revision.as_str(),
    )?;
    println!(
        "operation=stress phase=hub_save elapsed_ms={}",
        started.elapsed().as_millis()
    );
    ensure!(
        started.elapsed() < Duration::from_secs(1),
        "saving the million-link hub exceeded one second"
    );
    store.update(&hub, &original.body, changed.revision.as_str())?;
    let started = Instant::now();
    for _ in 0..4 {
        store.process_link_updates(256)?;
    }
    println!(
        "operation=stress phase=alias_updates elapsed_ms={}",
        started.elapsed().as_millis()
    );
    ensure!(
        started.elapsed() < Duration::from_secs(1),
        "unchanged aliases caused a collection scan"
    );
    let note = store.create(Some("stress-edit.md".into()), "# Temporary stress note")?;
    let started = Instant::now();
    let note = store.update(
        &note.name,
        "# Temporary stress note\n\nSaved under load.",
        note.revision.as_str(),
    )?;
    println!(
        "operation=stress phase=save elapsed_ms={}",
        started.elapsed().as_millis()
    );
    ensure!(
        started.elapsed() < Duration::from_secs(1),
        "saving exceeded one second"
    );
    store.delete(&note.name, note.revision.as_str())?;
    let record = store
        .trash()?
        .into_iter()
        .find(|record| record.name == note.name)
        .context("find deleted stress note")?;
    let restored = store.restore_trash(&record.id, None)?;
    ensure!(restored.body == note.body, "restoring changed the source");
    store.delete(&restored.name, restored.revision.as_str())?;
    store.empty_trash()?;
    let started = Instant::now();
    ensure!(
        store.process_recovery_cleanup(
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
            8
        )? <= 8,
        "cleanup exceeded its batch"
    );
    println!(
        "operation=stress phase=cleanup elapsed_ms={}",
        started.elapsed().as_millis()
    );
    ensure!(
        started.elapsed() < Duration::from_secs(1),
        "cleanup exceeded one second"
    );
    ensure!(
        Instant::now() < deadline,
        "inconclusive: stress checks exceeded the campaign deadline"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::Profile;

    #[test]
    fn extreme_fixtures_have_real_recovery_records_and_do_not_mutate_on_reuse() {
        let directory = tempfile::tempdir().unwrap();
        let corpus = Corpus::new(500, 42, Profile::Mixed).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        prepare(directory.path(), &corpus, deadline, 8).unwrap();
        let name = NoteName::parse(corpus.name(1)).unwrap();
        let before = Store::open(directory.path())
            .unwrap()
            .read(&name)
            .unwrap()
            .body;
        prepare(directory.path(), &corpus, deadline, 8).unwrap();
        assert_eq!(
            Store::open(directory.path())
                .unwrap()
                .read(&name)
                .unwrap()
                .body,
            before
        );
        storage_checks(directory.path(), &corpus.name(0), deadline).unwrap();
    }
}
