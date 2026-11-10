mod catalog;

use anyhow::{Context, Result, ensure};
use catalog::{DOMAINS, FACETS};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    path::Path,
    time::{Duration, Instant},
};

const TOPICS: usize = 400;
const MAPS: usize = DOMAINS.len() + TOPICS;

#[derive(Debug, PartialEq)]
pub(super) struct Note {
    pub(super) name: String,
    pub(super) title: String,
    pub(super) body: String,
    pub(super) links: Vec<usize>,
}

#[cfg(test)]
struct Notebook {
    notes: Vec<Note>,
    links: usize,
}

pub struct Report {
    pub notes: usize,
    pub links: usize,
    pub entry: String,
}

pub(super) struct Written {
    pub(super) links: usize,
    pub(super) bytes: u64,
    pub(super) allocated: u64,
}

pub(super) fn write_notes(
    directory: &Path,
    corpus: &Corpus,
    deadline: Instant,
    allocation_gib: u64,
) -> Result<Written> {
    use std::os::unix::fs::MetadataExt;
    use tungsten::{
        domain::{NoteName, Revision},
        store::{Note, Store},
    };
    let mut result = Written {
        links: 0,
        bytes: 0,
        allocated: 0,
    };
    let mut store = Store::open(directory)?;
    let imported = store.note_count()?;
    ensure!(
        imported <= corpus.count,
        "the partial fixture exceeds the selected corpus"
    );
    for index in [0, imported / 2, imported.saturating_sub(1)] {
        if imported == 0 {
            break;
        }
        let expected = corpus.note(index);
        ensure!(
            store.read(&NoteName::parse(expected.name)?)?.body == expected.body,
            "the partial fixture belongs to another corpus"
        );
    }
    store.import_notes((imported..corpus.count).map(|index| {
        if index.is_multiple_of(10_000) {
            if Instant::now() >= deadline {
                return Err(tungsten::Error::Limit(
                    "Dataset generation deadline exceeded.",
                ));
            }
            let allocated = fs::read_dir(directory)?.try_fold(0_u64, |total, entry| {
                Ok::<_, std::io::Error>(total + entry?.metadata()?.blocks() * 512)
            })?;
            if allocated > allocation_gib * 1024 * 1024 * 1024 {
                return Err(tungsten::Error::Limit(
                    "Dataset allocation exceeded its limit.",
                ));
            }
            eprintln!(
                "operation=demo_notes phase=write pages={index} total={} profile={}",
                corpus.count,
                corpus.profile.name()
            );
        }
        let note = corpus.note(index);
        result.links += note.links.len();
        result.bytes += note.body.len() as u64;
        Ok(Note {
            name: NoteName::parse(note.name)?,
            revision: Revision::of(note.body.as_bytes()),
            body: note.body,
            modified: 1_735_689_600_000
                + choose(corpus.seed, index, 30, 365 * 86400) as u128 * 1000,
        })
    }))?;
    drop(store);
    result.allocated = fs::read_dir(directory)?.try_fold(0_u64, |total, entry| {
        Ok::<_, std::io::Error>(total + entry?.metadata()?.blocks() * 512)
    })?;
    ensure!(
        result.allocated <= allocation_gib * 1024 * 1024 * 1024,
        "dataset allocation exceeded its limit"
    );
    Ok(result)
}

fn choose(seed: u64, index: usize, salt: u64, bound: usize) -> usize {
    let mut value = seed
        .wrapping_add((index as u64).wrapping_mul(0x9e3779b97f4a7c15))
        .wrapping_add(salt);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((value ^ (value >> 31)) % bound as u64) as usize
}

fn connections(index: usize, count: usize, seed: u64) -> Vec<usize> {
    let mut links = BTreeSet::new();
    if index < DOMAINS.len() {
        links.extend((0..20).map(|topic| DOMAINS.len() + index * 20 + topic));
        links.insert(DOMAINS[index].bridge);
        links.insert((index + 1) % DOMAINS.len());
    } else if index < MAPS {
        let topic = index - DOMAINS.len();
        let domain = topic / 20;
        links.insert(domain);
        links.insert(DOMAINS.len() + domain * 20 + (topic + 1) % 20);
        links.insert(DOMAINS.len() + DOMAINS[domain].bridge * 20 + topic % 20);
        links.extend((MAPS + topic..count).step_by(TOPICS));
    } else {
        let leaf = index - MAPS;
        let topic = leaf % TOPICS;
        let domain = topic / 20;
        let peers = (count - MAPS - topic).div_ceil(TOPICS);
        links.insert(DOMAINS.len() + topic);
        links.insert(DOMAINS.len() + domain * 20 + (topic + 1 + choose(seed, index, 1, 3)) % 20);
        for salt in [2, 3, 4] {
            let peer = choose(seed, index, salt, peers);
            links.insert(MAPS + topic + peer.min(choose(seed, index, salt + 10, peers)) * TOPICS);
        }
        if choose(seed, index, 5, 7) == 0 {
            let bridge_topic = DOMAINS[domain].bridge * 20 + choose(seed, index, 6, 20);
            links.insert(DOMAINS.len() + bridge_topic);
        }
        if choose(seed, index, 7, 31) == 0 {
            links.insert(choose(seed, index, 8, count));
        }
    }
    links.remove(&index);
    links.into_iter().collect()
}

#[derive(Clone, Copy, Debug, PartialEq, clap::ValueEnum)]
pub(super) enum Profile {
    Clustered,
    Mixed,
}

impl Profile {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Clustered => "clustered",
            Self::Mixed => "mixed",
        }
    }
}

pub(super) struct Corpus {
    pub(super) count: usize,
    pub(super) seed: u64,
    pub(super) profile: Profile,
    topics: Vec<String>,
}

impl Corpus {
    pub(super) fn new(count: usize, seed: u64, profile: Profile) -> Result<Self> {
        ensure!(
            (MAPS..=1_000_000).contains(&count),
            "count must be between {MAPS} and 1000000"
        );
        let mut topic_names = BTreeMap::new();
        for domain in &DOMAINS {
            for topic in domain.topics {
                *topic_names.entry(topic).or_insert(0) += 1;
            }
        }
        let topics: Vec<String> = DOMAINS
            .iter()
            .flat_map(|domain| {
                domain.topics.iter().map(|topic| {
                    if topic_names[topic] > 1 || DOMAINS.iter().any(|entry| entry.title == *topic) {
                        format!("{topic} ({})", domain.title)
                    } else {
                        (*topic).to_owned()
                    }
                })
            })
            .collect();

        Ok(Self {
            count,
            seed,
            profile,
            topics,
        })
    }

    pub(super) fn name(&self, index: usize) -> String {
        format!("{:016x}{index:016x}.md", self.seed)
    }

    fn domain(&self, index: usize) -> usize {
        if index < DOMAINS.len() {
            index
        } else if index < MAPS {
            (index - DOMAINS.len()) / 20
        } else {
            ((index - MAPS) % TOPICS) / 20
        }
    }

    pub(super) fn isolated(&self, index: usize) -> bool {
        self.profile == Profile::Mixed && index >= MAPS && choose(self.seed, index, 140, 100) < 3
    }

    pub(super) fn dangling(&self, index: usize, target: usize) -> bool {
        self.profile == Profile::Mixed
            && index >= MAPS
            && !self.isolated(index)
            && choose(self.seed, index, 146, 17) != 0
            && choose(self.seed, index, 151, 101) == 0
            && target == self.count + choose(self.seed, index, 152, 1000)
    }

    pub(super) fn title(&self, index: usize) -> String {
        if index >= self.count {
            return format!("Unwritten reference {}", index - self.count + 1);
        }
        if index < DOMAINS.len() {
            return DOMAINS[index].title.to_owned();
        }
        if index < MAPS {
            return self.topics[index - DOMAINS.len()].clone();
        }
        if self.profile == Profile::Mixed && choose(self.seed, index, 142, 53) == 0 {
            return "Reading notes".to_owned();
        }
        let leaf = index - MAPS;
        let visit = leaf / (TOPICS * FACETS.len());
        let mut title = format!(
            "{}: {}",
            self.topics[leaf % TOPICS],
            FACETS[(leaf / TOPICS) % FACETS.len()].title
        );
        if visit > 0 {
            title.push_str(&format!(" / Visit {}", visit + 1));
        }
        if self.profile == Profile::Mixed && choose(self.seed, index, 143, 37) == 0 {
            title.push_str(" / café 👩\u{200d}💻 日本語のメモ");
        }
        title
    }

    pub(super) fn note(&self, index: usize) -> Note {
        let title = self.title(index);
        let mut links = connections(index, self.count, self.seed);
        if self.profile == Profile::Mixed {
            links.retain(|&target| {
                !self.isolated(target)
                    && ((self.domain(index) == 19) == (self.domain(target) == 19))
            });
            if index >= MAPS && choose(self.seed, index, 145, 13) == 0 && self.domain(index) != 19 {
                links.push(0);
            }
            if self.isolated(index) || (index >= MAPS && choose(self.seed, index, 146, 17) == 0) {
                links.clear();
            } else if index >= MAPS && choose(self.seed, index, 151, 101) == 0 {
                links.push(self.count + choose(self.seed, index, 152, 1000));
            }
            links.sort_unstable();
            links.dedup();
        }
        let title = &title;
        let mut body = format!("# {title}\n\n");
        if index < DOMAINS.len() {
            let domain = &DOMAINS[index];
            body.push_str(&format!("This is a synthetic personal notebook about {}.\nThe pages are generated examples for exploring a connected notebook, not reference material.\n\nMy recurring question concerns {}.\nI use the topic maps to collect examples, disagreements, and small experiments rather than arranging every thought into a directory.\n\n## A route through the notebook\n\n{}.\nFollow a topic, inspect a particular case, and return with a sharper question.\n\n## Topics and neighboring subjects\n\n", domain.title.to_lowercase(), domain.lens, domain.exercise));
        } else if index < MAPS {
            let domain = &DOMAINS[(index - DOMAINS.len()) / 20];
            body.push_str(&format!("I am collecting questions about {title} alongside my notes on {}.\nThe useful starting point is {}.\n\n## Working questions\n\nWhat counts as a concrete example of {title}?\nWhich explanation survives a difficult case, and which assumptions need another look?\n\n{}.\nThe linked entries approach the subject through observations, comparisons, reading notes, and experiments.\n\n## Connected notes\n\n", domain.title.to_lowercase(), domain.lens, domain.exercise));
        } else {
            let leaf = index - MAPS;
            let topic = &self.topics[leaf % TOPICS];
            let domain = &DOMAINS[(leaf % TOPICS) / 20];
            let facet = &FACETS[(leaf / TOPICS) % FACETS.len()];
            body.push_str(&format!("Notebook entry: 2025-{:02}-{:02}.\n\nI am thinking about {topic} through {}.\n{}\n\n{}.\nI want to preserve the original question even if the explanation changes.\n\n", choose(self.seed, index, 20, 12) + 1, choose(self.seed, index, 21, 28) + 1, domain.lens, facet.question, facet.action));
            let observation = match choose(self.seed, index, 22, 6) {
                0 => {
                    "A neat explanation can hide the detail that made the example interesting.\nI need to describe the situation before deciding which concept explains it.\n"
                }
                1 => {
                    "The first interpretation feels familiar, but familiarity is not the same as evidence.\nI would like a second example with a different constraint.\n"
                }
                2 => {
                    "There may be two questions hidden in the same sentence.\nOne concerns the description of the situation, while the other concerns what to do about it.\n"
                }
                3 => {
                    "The practical case has a boundary that the abstract account leaves out.\nNaming the boundary may be more useful than adding another definition.\n"
                }
                4 => {
                    "A connection to a neighboring subject changed the question I wanted to ask.\nI have kept the link so that the reasoning can be reconstructed later.\n"
                }
                _ => {
                    "The useful result might be a better question rather than an immediate answer.\nI want to keep the example close to the explanation while revising the note.\n"
                }
            };
            body.push_str(observation);
            match choose(self.seed, index, 23, 8) {
                0 => body.push_str("\n## Next attempt\n\n- [ ] Find one concrete example.\n- [ ] Describe an exception.\n- [ ] Revisit the explanation after the comparison.\n"),
                1 => body.push_str("\n## A reminder\n\n> Keep the observation separate from the story used to explain it.\n"),
                2 => body.push_str("\n## Sketch\n\n```text\nobservation -> interpretation -> next question\n       ^                              |\n       +------------------------------+\n```\n"),
                3 => body.push_str("\n## Comparison\n\n| Approach | Question to ask |\n| --- | --- |\n| First account | What does it explain? |\n| Alternative | What does it leave out? |\n"),
                4 => body.push_str("\n## Working distinction\n\nThe **observation** is what I noticed.\nThe *interpretation* is an account that could still change.\nI am keeping `assumption`, `example`, and `conclusion` separate while revising.\n"),
                _ => {}
            }
            body.push_str("\n## Related notes\n\n");
        }

        if self.profile == Profile::Mixed && index >= MAPS {
            if choose(self.seed, index, 147, 211) == 0 {
                body.push_str("## Longer reflection\n\n");
                for _ in 0..24 {
                    body.push_str("The same example can be interpreted at more than one scale.\nI am keeping the observation, the question, and the proposed explanation separate while the note develops.\n\n");
                }
            }
            if choose(self.seed, index, 148, 23) == 0 {
                body.push_str("\n```md\n[Literal example](a-file-that-does-not-exist.md)\n```\n\n");
            }
            if choose(self.seed, index, 149, 101) == 0 {
                body = body
                    .split_once("\n\n")
                    .map_or(body.clone(), |(_, content)| content.to_owned());
            }
        }
        for &target in &links {
            body.push_str(&format!(
                "- [{}]({})\n",
                self.title(target),
                self.name(target)
            ));
        }
        Note {
            name: self.name(index),
            title: title.clone(),
            body,
            links,
        }
    }
}

#[cfg(test)]
fn generate(count: usize, seed: u64) -> Result<Notebook> {
    ensure!(count <= 10_000, "unit fixtures are limited to 10000 pages");
    let corpus = Corpus::new(count, seed, Profile::Clustered)?;
    let notes: Vec<_> = (0..count).map(|index| corpus.note(index)).collect();
    let links = notes.iter().map(|note| note.links.len()).sum();
    Ok(Notebook { notes, links })
}

pub fn create(output: &Path, count: usize, seed: u64) -> Result<Report> {
    ensure!(
        fs::symlink_metadata(output)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound),
        "destination must not exist"
    );
    ensure!(
        count <= 10_000,
        "use the bounded scale-profile command above 10000 pages"
    );
    let corpus = Corpus::new(count, seed, Profile::Clustered)?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".tungsten-demo-")
        .tempdir_in(parent)?;
    eprintln!("operation=demo_notes phase=write notes={count} seed={seed}");
    let written = write_notes(
        staging.path(),
        &corpus,
        Instant::now() + Duration::from_secs(60),
        8,
    )?;
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        staging.path(),
        rustix::fs::CWD,
        output,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .context("publish sample notebook without replacing an existing path")?;
    File::open(parent)?.sync_all()?;
    Ok(Report {
        notes: count,
        links: written.links,
        entry: corpus.name(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulldown_cmark::{Event, Parser, Tag};
    use std::collections::BTreeSet;

    #[test]
    fn partial_notebooks_resume_without_replacing_committed_notes() {
        use tungsten::{
            domain::{NoteName, Revision},
            store::{Note, Store},
        };
        let directory = tempfile::tempdir().unwrap();
        let corpus = Corpus::new(600, 42, Profile::Mixed).unwrap();
        let mut store = Store::open(directory.path()).unwrap();
        assert!(
            store
                .import_notes((0..502).map(|index| {
                    if index == 501 {
                        return Err(tungsten::Error::Worker);
                    }
                    let note = corpus.note(index);
                    Ok(Note {
                        name: NoteName::parse(note.name)?,
                        revision: Revision::of(note.body.as_bytes()),
                        body: note.body,
                        modified: 1,
                    })
                }))
                .is_err()
        );
        drop(store);
        write_notes(
            directory.path(),
            &corpus,
            Instant::now() + Duration::from_secs(20),
            8,
        )
        .unwrap();
        let store = Store::open(directory.path()).unwrap();
        assert_eq!(store.search("", None, 1, false).unwrap().total, 600);
        assert_eq!(
            store
                .read(&NoteName::parse(corpus.name(0)).unwrap())
                .unwrap()
                .modified,
            1
        );
        assert_eq!(
            store
                .read(&NoteName::parse(corpus.name(599)).unwrap())
                .unwrap()
                .body,
            corpus.note(599).body
        );
        drop(store);
        let unrelated = Corpus::new(600, 43, Profile::Mixed).unwrap();
        assert!(
            write_notes(
                directory.path(),
                &unrelated,
                Instant::now() + Duration::from_secs(20),
                8
            )
            .is_err()
        );
    }

    #[test]
    fn notebook_has_ten_thousand_distinct_pages_and_resolvable_semantic_links() {
        let notebook = generate(10_000, 42).unwrap();
        assert_eq!(notebook.notes.len(), 10_000);
        let names: BTreeSet<_> = notebook
            .notes
            .iter()
            .map(|note| note.name.as_str())
            .collect();
        let titles: BTreeSet<_> = notebook
            .notes
            .iter()
            .map(|note| note.title.as_str())
            .collect();
        assert_eq!(names.len(), 10_000);
        assert_eq!(titles.len(), 10_000);
        assert_eq!(notebook.notes[0].title, "Philosophy");
        let mut links = 0;
        for note in &notebook.notes {
            assert!(note.body.starts_with(&format!("# {}\n\n", note.title)));
            assert!(note.body.len() > 300 && note.body.len() < 20_000);
            let mut seen = BTreeSet::new();
            for event in Parser::new(&note.body) {
                if let Event::Start(Tag::Link { dest_url, .. }) = event {
                    assert!(
                        names.contains(dest_url.as_ref()),
                        "{} -> {dest_url}",
                        note.name
                    );
                    assert_ne!(dest_url.as_ref(), note.name);
                    seen.insert(dest_url.into_string());
                    links += 1;
                }
            }
            assert!(!seen.is_empty());
            assert_eq!(seen.len(), note.links.len());
        }
        assert!(links > 40_000 && links < 100_000);
        assert_eq!(links, notebook.links);
        let ethics = notebook
            .notes
            .iter()
            .position(|note| note.title == "Ethics")
            .unwrap();
        assert!(notebook.notes[0].links.contains(&ethics));
        assert!(notebook.notes[ethics].links.len() > 20);
    }

    #[test]
    fn seeds_reproduce_content_and_count_boundaries_keep_links_valid() {
        for count in [420, 421, 500, 9_999] {
            let notebook = generate(count, 7).unwrap();
            assert_eq!(notebook.notes.len(), count);
            for note in &notebook.notes {
                assert!(note.links.iter().all(|&index| index < count));
            }
        }
        assert_eq!(
            generate(500, 42).unwrap().notes,
            generate(500, 42).unwrap().notes
        );
        assert_ne!(
            generate(500, 42).unwrap().notes,
            generate(500, 43).unwrap().notes
        );
        assert!(generate(419, 42).is_err());
        assert!(generate(10_001, 42).is_err());
    }

    #[test]
    fn published_notebooks_never_replace_an_existing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("notebook");
        let report = create(&destination, 500, 42).unwrap();
        assert_eq!(report.notes, 500);
        assert_eq!(
            tungsten::store::Store::open(&destination)
                .unwrap()
                .search("", None, 1, false)
                .unwrap()
                .total,
            500
        );
        let name = tungsten::domain::NoteName::parse(report.entry.clone()).unwrap();
        let entry = tungsten::store::Store::open(&destination)
            .unwrap()
            .read(&name)
            .unwrap()
            .body;
        assert!(create(&destination, 500, 43).is_err());
        assert_eq!(
            tungsten::store::Store::open(&destination)
                .unwrap()
                .read(&name)
                .unwrap()
                .body,
            entry
        );
        let empty = directory.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        assert!(create(&empty, 500, 42).is_err());
        assert_eq!(std::fs::read_dir(&empty).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
    }

    #[test]
    fn million_page_corpora_generate_pages_on_demand_with_varied_topology() {
        let corpus = Corpus::new(1_000_000, 42, Profile::Mixed).unwrap();
        for index in [0, 20, 419, 420, 9_999, 10_000, 999_999] {
            let note = corpus.note(index);
            assert!(note.body.len() < 1024 * 1024);
            assert!(
                note.links
                    .iter()
                    .all(|&target| target < 1_000_000 || corpus.dangling(index, target))
            );
        }
        let mut isolated = 0;
        let mut terminal = 0;
        let mut duplicate_titles = 0;
        let mut dangling = 0;
        for index in 420..10_420 {
            let note = corpus.note(index);
            isolated += usize::from(corpus.isolated(index));
            terminal += usize::from(note.links.is_empty());
            duplicate_titles += usize::from(note.title == "Reading notes");
            dangling += usize::from(note.links.iter().any(|&target| target >= corpus.count));
        }
        assert!(isolated > 100 && isolated < 500);
        assert!(terminal > isolated);
        assert!(duplicate_titles > 10);
        assert!(dangling > 0);
        assert_ne!(corpus.title(420), corpus.title(10_020));
        assert!(Corpus::new(1_000_001, 42, Profile::Mixed).is_err());
    }

    #[test]
    fn intentional_missing_links_allow_only_the_generated_destination() {
        let corpus = Corpus::new(10_000, 42, Profile::Mixed).unwrap();
        let (source, destination) = (MAPS..corpus.count)
            .find_map(|index| {
                corpus
                    .note(index)
                    .links
                    .into_iter()
                    .find(|&target| target >= corpus.count)
                    .map(|target| (index, target))
            })
            .unwrap();
        assert!(corpus.dangling(source, destination));
        assert!(!corpus.dangling(source, destination + 1_000));
        assert!(!corpus.dangling(source, source));
    }
}
