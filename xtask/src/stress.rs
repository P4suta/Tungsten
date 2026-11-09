use crate::demo::Corpus;
use anyhow::{Context, Result, ensure};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::MetadataExt,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tungsten::{
    domain::{NoteName, Revision},
    store::Store,
};

fn write_once(path: &Path, bytes: &[u8]) -> Result<()> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file.write_all(bytes)?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure!(fs::read(path)? == bytes, "existing stress fixture differs")
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn link_once(source: &Path, target: &Path) -> Result<()> {
    match fs::hard_link(source, target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure!(
                fs::metadata(source)?.ino() == fs::metadata(target)?.ino(),
                "existing recovery fixture differs"
            );
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

pub(super) fn prepare(directory: &Path, corpus: &Corpus, deadline: Instant) -> Result<()> {
    fs::create_dir_all(directory)?;
    let clock = directory.join(".tungsten-stress-clock");
    let captured = match fs::read_to_string(&clock) {
        Ok(text) => text.parse::<u128>()?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
            write_once(&clock, now.to_string().as_bytes())?;
            now
        }
        Err(error) => return Err(error.into()),
    };
    let history = directory.join(".history");
    fs::create_dir_all(&history)?;
    let hub = corpus.name(0);
    let variants = corpus.count.min(4096);
    let mut allocated = 0;
    for index in 0..corpus.count {
        if index.is_multiple_of(10_000) {
            ensure!(
                Instant::now() < deadline,
                "inconclusive: stress generation deadline exceeded"
            );
            eprintln!(
                "operation=stress phase=generate records={index} total={}",
                corpus.count
            );
        }
        let path = directory.join(corpus.name(index));
        let body = if index == 0 {
            corpus.note(0).body
        } else {
            format!("# Stress note {index}\n\nUnicode 日本語.\n\n[Scale hub]({hub})\n")
        };
        write_once(&path, body.as_bytes())?;
        allocated += fs::metadata(&path)?.blocks() * 512;
        let record = history.join(format!("{index:032x}"));
        fs::create_dir_all(&record)?;
        if index < variants {
            let body = format!("# Snapshot {index}\n\nSynthetic recovery record 日本語.\n");
            let revision = Revision::of(body.as_bytes());
            let metadata = serde_json::to_vec(
                &serde_json::json!({"name":hub,"modified":captured,"captured":captured.saturating_sub(index as u128),"revision":revision.as_str()}),
            )?;
            write_once(&record.join("body.md"), body.as_bytes())?;
            write_once(&record.join("metadata.json"), &metadata)?;
            allocated += fs::metadata(record.join("body.md"))?.blocks() * 512;
            allocated += fs::metadata(record.join("metadata.json"))?.blocks() * 512;
        } else {
            let template = history.join(format!("{:032x}", index % variants));
            for name in ["body.md", "metadata.json"] {
                link_once(&template.join(name), &record.join(name))?;
            }
        }
        allocated += fs::metadata(&record)?.blocks() * 512;
        ensure!(
            allocated <= 12 * 1024 * 1024 * 1024,
            "inconclusive: stress allocation exceeded 12 GiB"
        );
    }
    let generated = fs::read_dir(&history)?
        .filter_map(Result::ok)
        .filter(|entry| {
            u128::from_str_radix(&entry.file_name().to_string_lossy(), 16)
                .is_ok_and(|id| id < corpus.count as u128)
        })
        .count();
    ensure!(generated == corpus.count, "unexpected stress history count");
    write_once(
        &directory.join(".tungsten-extremes"),
        corpus.count.to_string().as_bytes(),
    )
    .context("publish the stress fixture")?;
    println!(
        "operation=stress phase=data_verified notes={} hub_incoming={} recovery_records={} body_variants={variants} allocated_bytes={allocated}",
        corpus.count,
        corpus.count - 1,
        corpus.count
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
        prepare(directory.path(), &corpus, deadline).unwrap();
        let before = fs::read(directory.path().join(corpus.name(1))).unwrap();
        prepare(directory.path(), &corpus, deadline).unwrap();
        assert_eq!(
            fs::read(directory.path().join(corpus.name(1))).unwrap(),
            before
        );
        storage_checks(directory.path(), &corpus.name(0), deadline).unwrap();
    }
}
