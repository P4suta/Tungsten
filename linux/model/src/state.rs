#![deny(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Block {
    None,
    Conflict,
    Missing,
    Uncertain,
}

#[cfg(kani)]
mod proofs {
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Acknowledged,
    Conflict,
    Missing,
    Uncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ticket {
    scope: u64,
    edit: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct State {
    scope: u64,
    edit: u64,
    durable: Option<u64>,
    saved: Option<u64>,
    pending: Option<Ticket>,
    block: Block,
}

impl State {
    pub const fn loaded(scope: u64) -> Self {
        Self {
            scope,
            edit: 0,
            durable: Some(0),
            saved: Some(0),
            pending: None,
            block: Block::None,
        }
    }

    pub const fn draft(scope: u64) -> Self {
        Self {
            scope,
            edit: 0,
            durable: None,
            saved: None,
            pending: None,
            block: Block::None,
        }
    }

    pub fn edited(self) -> Option<Self> {
        Some(Self {
            edit: self.edit.checked_add(1)?,
            ..self
        })
    }

    pub const fn persisted(self) -> Self {
        Self {
            durable: Some(self.edit),
            ..self
        }
    }

    pub const fn blocked(self, block: Block) -> Self {
        Self { block, ..self }
    }

    pub fn begin(self) -> (Self, Option<Ticket>) {
        if self.durable != Some(self.edit)
            || self.saved == Some(self.edit)
            || self.pending.is_some()
            || self.block != Block::None
        {
            return (self, None);
        }
        let ticket = Ticket {
            scope: self.scope,
            edit: self.edit,
        };
        (
            Self {
                pending: Some(ticket),
                ..self
            },
            Some(ticket),
        )
    }

    pub fn complete(self, ticket: Ticket, outcome: Outcome) -> Self {
        if self.pending != Some(ticket) || self.scope != ticket.scope {
            return self;
        }
        let (saved, block) = match outcome {
            Outcome::Acknowledged => (Some(ticket.edit), Block::None),
            Outcome::Conflict => (self.saved, Block::Conflict),
            Outcome::Missing => (self.saved, Block::Missing),
            Outcome::Uncertain => (self.saved, Block::Uncertain),
        };
        Self {
            saved,
            block,
            pending: None,
            ..self
        }
    }

    pub fn accepts(self, ticket: Ticket) -> bool {
        self.pending == Some(ticket) && self.scope == ticket.scope
    }

    pub fn is_saved(self) -> bool {
        self.saved == Some(self.edit) && self.pending.is_none() && self.block == Block::None
    }

    pub fn may_leave(self) -> bool {
        self.durable == Some(self.edit)
    }

    pub const fn block(self) -> Block {
        self.block
    }

    pub const fn pending(self) -> Option<Ticket> {
        self.pending
    }
}
