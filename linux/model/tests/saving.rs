use tungsten_linux_model::{Block, Outcome, State};

#[test]
fn edits_must_be_durable_before_sending() {
    let initial = State::draft(1);
    assert!(initial.begin().1.is_none());
    let durable = initial.persisted();
    let (writing, ticket) = durable.begin();
    assert!(ticket.is_some());
    assert!(writing.begin().1.is_none());
}

#[test]
fn acknowledgement_preserves_edits_during_a_write() {
    let (writing, ticket) = State::draft(1).persisted().begin();
    let ticket = ticket.unwrap();
    let edited = writing.edited().unwrap();
    let saved = edited.complete(ticket, Outcome::Acknowledged);
    assert!(!saved.is_saved());
    assert!(!saved.may_leave());
    assert!(saved.begin().1.is_none());
    let (writing, ticket) = saved.persisted().begin();
    assert!(
        writing
            .complete(ticket.unwrap(), Outcome::Acknowledged)
            .is_saved()
    );
}

#[test]
fn conflicts_missing_notes_and_uncertain_delivery_stop_autosave() {
    for (outcome, block) in [
        (Outcome::Conflict, Block::Conflict),
        (Outcome::Missing, Block::Missing),
        (Outcome::Uncertain, Block::Uncertain),
    ] {
        let (writing, ticket) = State::draft(1).persisted().begin();
        let blocked = writing.complete(ticket.unwrap(), outcome);
        assert_eq!(blocked.block(), block);
        assert!(blocked.begin().1.is_none());
        assert!(!blocked.is_saved());
        assert!(blocked.may_leave());
        assert!(blocked.edited().unwrap().persisted().begin().1.is_none());
    }
}

#[test]
fn delayed_responses_cannot_change_another_editor() {
    let (_, ticket) = State::draft(1).persisted().begin();
    let current = State::loaded(2).edited().unwrap().persisted();
    for result in [
        Outcome::Acknowledged,
        Outcome::Conflict,
        Outcome::Missing,
        Outcome::Uncertain,
    ] {
        assert_eq!(current.complete(ticket.unwrap(), result), current);
    }
}

#[test]
fn local_failure_prevents_navigation_and_exit() {
    let loaded = State::loaded(1);
    assert!(loaded.is_saved());
    let edited = loaded.edited().unwrap();
    assert!(!edited.may_leave());
    assert!(!edited.is_saved());
    assert!(edited.persisted().may_leave());
}

#[test]
fn explicit_blocks_stop_writes_until_cleared() {
    for block in [Block::Conflict, Block::Missing, Block::Uncertain] {
        let clean = State::loaded(1).blocked(block);
        assert_eq!(clean.block(), block);
        assert!(!clean.is_saved());
        let draft = State::draft(1).persisted().blocked(block);
        assert_eq!(draft.block(), block);
        assert!(draft.may_leave());
        assert!(draft.begin().1.is_none());
        let resumed = draft.blocked(Block::None);
        assert_eq!(resumed.block(), Block::None);
        assert!(resumed.begin().1.is_some());
    }
}

#[test]
fn tickets_identify_only_the_current_pending_write() {
    let idle = State::draft(1).persisted();
    let (writing, ticket) = idle.begin();
    let ticket = ticket.unwrap();
    let (_, other) = State::draft(2).persisted().begin();
    let other = other.unwrap();
    assert!(idle.pending().is_none());
    assert!(!idle.accepts(ticket));
    assert_eq!(writing.pending(), Some(ticket));
    assert!(writing.accepts(ticket));
    assert!(!writing.accepts(other));
    let saved = writing.complete(ticket, Outcome::Acknowledged);
    assert!(saved.pending().is_none());
    assert!(!saved.accepts(ticket));
    let (next, current) = saved.edited().unwrap().persisted().begin();
    let current = current.unwrap();
    assert_ne!(current, ticket);
    assert_eq!(next.pending(), Some(current));
    assert!(next.accepts(current));
    assert!(!next.accepts(ticket));
    for outcome in [
        Outcome::Acknowledged,
        Outcome::Conflict,
        Outcome::Missing,
        Outcome::Uncertain,
    ] {
        assert_eq!(writing.complete(other, outcome), writing);
        assert_eq!(next.complete(ticket, outcome), next);
    }
    assert!(next.complete(current, Outcome::Acknowledged).is_saved());
}
