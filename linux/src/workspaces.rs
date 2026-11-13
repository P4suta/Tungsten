use std::collections::BTreeMap;

use crate::{Error, Result, types::Workspace};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceObservation(u64);

#[derive(Default)]
pub struct WorkspaceCache {
    observation: WorkspaceObservation,
    entries: BTreeMap<String, (WorkspaceObservation, Option<Workspace>)>,
}

impl WorkspaceCache {
    pub fn snapshot(&self) -> WorkspaceObservation {
        self.observation
    }

    pub fn observe(
        &mut self,
        workspace: Workspace,
        started: WorkspaceObservation,
    ) -> Result<Option<Workspace>> {
        if let Some((changed, current)) = self.entries.get(workspace.id.as_str()) {
            if changed.0 > started.0 {
                return Ok(current.clone());
            }
            if current
                .as_ref()
                .is_some_and(|current| current.revision == workspace.revision)
            {
                return Ok(current.clone());
            }
        }
        self.advance()?;
        self.entries.insert(
            workspace.id.as_str().to_owned(),
            (self.observation, Some(workspace.clone())),
        );
        Ok(Some(workspace))
    }

    pub fn remove(&mut self, id: &crate::types::WorkspaceId) -> Result<()> {
        self.advance()?;
        self.entries
            .insert(id.as_str().to_owned(), (self.observation, None));
        Ok(())
    }

    fn advance(&mut self) -> Result<()> {
        self.observation =
            WorkspaceObservation(self.observation.0.checked_add(1).ok_or(Error::Generation)?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Revision, types::WorkspaceId};

    fn workspace(id: &str, revision: &str) -> Workspace {
        Workspace {
            id: WorkspaceId::try_from(id.to_owned()).unwrap(),
            title: revision.to_owned(),
            revision: Revision::of(revision.as_bytes()),
        }
    }

    #[test]
    fn overlapping_response_keeps_the_observed_revision() {
        let mut cache = WorkspaceCache::default();
        let pending = cache.snapshot();
        cache.observe(workspace("1", "b"), pending).unwrap();
        let returned = cache
            .observe(workspace("1", "a"), pending)
            .unwrap()
            .unwrap();
        assert_eq!(returned.revision, workspace("1", "b").revision);
        assert_eq!(returned.title, "b");
    }

    #[test]
    fn a_read_started_after_the_observation_can_update_the_record() {
        let mut cache = WorkspaceCache::default();
        cache
            .observe(workspace("1", "a"), cache.snapshot())
            .unwrap();
        let pending = cache.snapshot();
        let returned = cache
            .observe(workspace("1", "b"), pending)
            .unwrap()
            .unwrap();
        assert_eq!(returned.revision, workspace("1", "b").revision);
    }

    #[test]
    fn different_records_accept_responses_from_the_same_snapshot() {
        let mut cache = WorkspaceCache::default();
        let pending = cache.snapshot();
        cache.observe(workspace("1", "a"), pending).unwrap();
        let returned = cache
            .observe(workspace("2", "b"), pending)
            .unwrap()
            .unwrap();
        assert_eq!(returned.id.as_str(), "2");
    }

    #[test]
    fn unchanged_metadata_does_not_invalidate_an_overlapping_update() {
        let mut cache = WorkspaceCache::default();
        cache
            .observe(workspace("1", "a"), cache.snapshot())
            .unwrap();
        let pending = cache.snapshot();
        cache.observe(workspace("1", "a"), pending).unwrap();
        assert_eq!(cache.snapshot(), pending);
        let returned = cache
            .observe(workspace("1", "b"), pending)
            .unwrap()
            .unwrap();
        assert_eq!(returned.revision, workspace("1", "b").revision);
    }

    #[test]
    fn deletion_rejects_a_pending_response() {
        let mut cache = WorkspaceCache::default();
        let pending = cache.snapshot();
        cache.remove(&workspace("1", "a").id).unwrap();
        assert!(
            cache
                .observe(workspace("1", "a"), pending)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn exhausted_observations_do_not_replace_a_record() {
        let mut cache = WorkspaceCache::default();
        cache
            .observe(workspace("1", "a"), cache.snapshot())
            .unwrap();
        cache.observation = WorkspaceObservation(u64::MAX);
        assert!(matches!(
            cache.observe(workspace("1", "b"), cache.snapshot()),
            Err(Error::Generation)
        ));
        assert!(matches!(
            cache.remove(&workspace("1", "a").id),
            Err(Error::Generation)
        ));
        assert_eq!(cache.entries["1"].1.as_ref().unwrap().title, "a");
    }
}
