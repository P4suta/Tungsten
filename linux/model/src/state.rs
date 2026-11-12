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
#[path = "proofs.rs"]
mod proofs;

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
        if !self.accepts(ticket) {
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
        self.pending == Some(ticket)
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
