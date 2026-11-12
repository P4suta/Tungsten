use super::*;

#[kani::proof]
#[kani::unwind(2)]
fn linux_transmission_requires_a_durable_draft() {
    let persisted: bool = kani::any();
    let state = if persisted {
        State::draft(0).persisted()
    } else {
        State::draft(0)
    };
    let (writing, ticket) = state.begin();
    assert_eq!(ticket.is_some(), persisted);
    assert!(writing.begin().1.is_none());
    kani::cover!(persisted);
    kani::cover!(!persisted);
}

#[kani::proof]
#[kani::unwind(2)]
fn linux_acknowledgements_preserve_later_edits() {
    let edit: bool = kani::any();
    let (writing, ticket) = State::draft(0).persisted().begin();
    assert!(ticket.is_some());
    let Some(ticket) = ticket else {
        return;
    };
    let current = if edit {
        writing.edited()
    } else {
        Some(writing)
    };
    assert!(current.is_some());
    let Some(current) = current else {
        return;
    };
    let complete = current.complete(ticket, Outcome::Acknowledged);
    assert_eq!(complete.is_saved(), !edit);
    assert_eq!(complete.may_leave(), !edit);
    assert!(complete.pending().is_none());
    kani::cover!(edit);
    kani::cover!(!edit);
}

#[kani::proof]
#[kani::unwind(2)]
fn linux_failures_stop_automatic_writes() {
    let failure: u8 = kani::any();
    let (writing, ticket) = State::draft(0).persisted().begin();
    assert!(ticket.is_some());
    let Some(ticket) = ticket else {
        return;
    };
    let outcome = match failure {
        0 => Outcome::Conflict,
        1 => Outcome::Missing,
        _ => Outcome::Uncertain,
    };
    let blocked = writing.complete(ticket, outcome);
    assert!(blocked.begin().1.is_none());
    assert!(!blocked.is_saved());
    assert!(blocked.may_leave());
    kani::cover!(failure == 0);
    kani::cover!(failure == 1);
    kani::cover!(failure == 2);
}

#[kani::proof]
#[kani::unwind(2)]
fn linux_editor_scopes_reject_delayed_responses() {
    let acknowledged: bool = kani::any();
    let (_, ticket) = State::draft(0).persisted().begin();
    assert!(ticket.is_some());
    let Some(ticket) = ticket else {
        return;
    };
    let (current, _) = State::draft(1).persisted().begin();
    let result = if acknowledged {
        Outcome::Acknowledged
    } else {
        Outcome::Conflict
    };
    assert_eq!(current.complete(ticket, result), current);
    kani::cover!(acknowledged);
    kani::cover!(!acknowledged);
}
