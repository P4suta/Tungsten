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
