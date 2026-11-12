use crate::{
    Error, Result,
    drafts::{Draft, DraftStore, PendingWrite},
    types::{Note, ServerAddress, generated_name},
};
use tungsten_linux_model::{Block, Outcome, State, Ticket};

pub struct Editor {
    draft: Draft,
    state: State,
}

#[derive(Clone)]
pub struct HistoryRead {
    state: State,
    name: crate::NoteName,
}

impl Editor {
    pub fn loaded(server: ServerAddress, note: Note, scope: u64) -> Self {
        Self {
            draft: Draft {
                format: 1,
                server,
                name: note.name,
                body: note.body,
                revision: Some(note.revision),
                pending: None,
            },
            state: State::loaded(scope),
        }
    }

    pub fn create(server: ServerAddress, scope: u64) -> Self {
        Self {
            draft: Draft {
                format: 1,
                server,
                name: generated_name(),
                body: String::new(),
                revision: None,
                pending: None,
            },
            state: State::draft(scope),
        }
    }

    pub fn recover(draft: Draft, scope: u64) -> Self {
        let state = State::draft(scope)
            .persisted()
            .blocked(if draft.pending.is_some() {
                Block::Uncertain
            } else {
                Block::None
            });
        Self { draft, state }
    }

    pub fn draft(&self) -> &Draft {
        &self.draft
    }
    pub fn state(&self) -> State {
        self.state
    }

    pub fn edit(&mut self, body: String, store: &DraftStore) -> Result<()> {
        if body == self.draft.body {
            return Ok(());
        }
        self.state = self.state.edited().ok_or(Error::Generation)?;
        self.draft.body = body;
        self.persist(store)
    }

    pub fn persist(&mut self, store: &DraftStore) -> Result<()> {
        if self.state.is_saved() {
            return store.remove(&self.draft.server, &self.draft.name);
        }
        store.save(&self.draft)?;
        self.state = self.state.persisted();
        Ok(())
    }

    pub fn begin(&mut self, store: &DraftStore) -> Result<Option<(Ticket, Draft)>> {
        let (state, ticket) = self.state.begin();
        let Some(ticket) = ticket else {
            return Ok(None);
        };
        self.draft.pending = Some(PendingWrite {
            body: self.draft.body.clone(),
            revision: self.draft.revision.clone(),
        });
        store.save(&self.draft)?;
        self.state = state;
        Ok(Some((ticket, self.draft.clone())))
    }

    pub fn complete(
        &mut self,
        ticket: Ticket,
        result: Result<Note>,
        store: &DraftStore,
    ) -> Result<bool> {
        if !self.state.accepts(ticket) {
            return Ok(false);
        }
        match result {
            Ok(note) => {
                if let Err(error) = note.validate(&self.draft.name) {
                    self.state = self.state.complete(ticket, Outcome::Uncertain);
                    return Err(error);
                }
                if self
                    .draft
                    .pending
                    .as_ref()
                    .is_none_or(|write| write.body != note.body)
                {
                    self.state = self.state.complete(ticket, Outcome::Uncertain);
                    return Err(Error::Response);
                }
                self.state = self.state.complete(ticket, Outcome::Acknowledged);
                self.draft.revision = Some(note.revision);
                self.draft.pending = None;
                if let Err(error) = store.save(&self.draft) {
                    self.state = self.state.blocked(Block::Uncertain);
                    return Err(error);
                }
                if self.state.is_saved() {
                    store.remove(&self.draft.server, &self.draft.name)?;
                }
                Ok(true)
            }
            Err(error) => {
                let outcome = match error {
                    Error::Conflict | Error::Exists => Outcome::Conflict,
                    Error::Missing => Outcome::Missing,
                    _ => Outcome::Uncertain,
                };
                self.state = self.state.complete(ticket, outcome);
                Err(error)
            }
        }
    }

    pub fn reconcile(&mut self, remote: Result<Note>, store: &DraftStore) -> Result<()> {
        if self.state.pending().is_some() {
            return Err(Error::Busy);
        }
        let remote = match remote {
            Ok(note) => note,
            Err(Error::Missing)
                if self.draft.revision.is_none() && self.draft.pending.is_none() =>
            {
                self.state = self.state.blocked(Block::None);
                return self.persist(store);
            }
            Err(error) => {
                self.state = self.state.blocked(if matches!(error, Error::Missing) {
                    Block::Missing
                } else {
                    Block::Uncertain
                });
                return Err(error);
            }
        };
        remote.validate(&self.draft.name)?;
        let committed = self
            .draft
            .pending
            .as_ref()
            .is_some_and(|write| write.body == remote.body);
        if self.draft.revision.as_ref() == Some(&remote.revision)
            || committed
            || self.draft.body == remote.body
        {
            self.draft.revision = Some(remote.revision);
            self.draft.pending = None;
            self.state = self.state.blocked(Block::None);
            self.persist(store)
        } else {
            self.state = self.state.blocked(Block::Conflict);
            Err(Error::Conflict)
        }
    }

    pub fn copy(&self, scope: u64, store: &DraftStore) -> Result<Self> {
        if !self.state.may_leave() {
            return Err(Error::Storage(std::io::Error::other(
                "draft is not durable",
            )));
        }
        let mut copy = Self::create(self.draft.server.clone(), scope);
        copy.draft.body.clone_from(&self.draft.body);
        copy.persist(store)?;
        Ok(copy)
    }

    pub fn history_read(&self) -> HistoryRead {
        HistoryRead {
            state: self.state,
            name: self.draft.name.clone(),
        }
    }

    pub fn replace_from_history(
        &mut self,
        read: &HistoryRead,
        body: String,
        store: &DraftStore,
    ) -> Result<()> {
        if self.state != read.state
            || self.draft.name != read.name
            || self.state.pending().is_some()
            || !self.state.may_leave()
        {
            return Err(Error::Busy);
        }
        let previous = self.draft.clone();
        let result = self.edit(body, store);
        if result.is_err() {
            self.draft = previous;
        }
        result
    }
}
