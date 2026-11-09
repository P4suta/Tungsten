include!("../../target/browser-proof.rs");

#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    #[kani::unwind(2)]
    fn browser_save_permissions() {
        let (dirty, busy, composing, blocked): (bool, bool, bool, bool) = kani::any();
        let allowed = may_save(dirty, busy, composing, blocked);
        assert_eq!(allowed, dirty && !busy && !composing && !blocked);
        kani::cover!(allowed, "dirty idle input can save");
        kani::cover!(!allowed, "unsafe input cannot save");
    }

    #[kani::proof]
    #[kani::unwind(2)]
    fn browser_saved_requires_acknowledgement() {
        let (same_text, has_revision, busy): (bool, bool, bool) = kani::any();
        let saved = is_saved(same_text, has_revision, busy);
        assert_eq!(saved, same_text && has_revision && !busy);
        kani::cover!(saved, "acknowledged unchanged text is saved");
        kani::cover!(!saved, "unacknowledged or busy text is not saved");
    }

    #[kani::proof]
    #[kani::unwind(2)]
    fn browser_refresh_requires_a_visible_idle_note() {
        let (visible, note, loading, pending, operation, composing, blocked): (
            bool,
            bool,
            bool,
            bool,
            bool,
            bool,
            bool,
        ) = kani::any();
        let allowed = may_read_remote(
            visible, note, loading, pending, operation, composing, blocked,
        );
        assert_eq!(
            allowed,
            visible && note && !(loading || pending || operation || composing || blocked)
        );
        kani::cover!(allowed, "visible idle notes can refresh");
        kani::cover!(!allowed, "unsafe refreshes are refused");
    }

    #[kani::proof]
    #[kani::unwind(2)]
    fn browser_refresh_requires_the_current_revision_and_editor() {
        let (editor, generation, revision, loading, pending, operation, composing, blocked): (
            bool,
            bool,
            bool,
            bool,
            bool,
            bool,
            bool,
            bool,
        ) = kani::any();
        let allowed = may_apply_remote(
            editor, generation, revision, loading, pending, operation, composing, blocked,
        );
        assert_eq!(
            allowed,
            editor
                && generation
                && revision
                && !(loading || pending || operation || composing || blocked)
        );
        kani::cover!(allowed, "current idle replies can apply");
        kani::cover!(!allowed, "stale or unsafe replies cannot apply");
    }

    #[kani::proof]
    #[kani::unwind(2)]
    fn browser_remote_conflicts_preserve_different_local_text() {
        let (body, saved, remote): (u16, u16, u16) = kani::any();
        let conflict = has_remote_conflict(body != saved, body == remote);
        assert_eq!(conflict, body != saved && body != remote);
        let result = if conflict { body } else { remote };
        if body != saved {
            assert_eq!(result, body);
        }
        kani::cover!(conflict, "different local writing is preserved");
        kani::cover!(
            !conflict && body != saved,
            "identical concurrent writing can be acknowledged"
        );
        kani::cover!(
            !conflict && body == saved && body != remote,
            "clean text can adopt remote writing"
        );
    }

    #[kani::proof]
    #[kani::unwind(2)]
    fn browser_draft_recovery_respects_living_owners() {
        let (matches, legacy, coordinated, active): (bool, bool, bool, bool) = kani::any();
        let allowed = may_recover_draft(matches, legacy, coordinated, active);
        assert_eq!(allowed, matches && (legacy || (coordinated && !active)));
        if !legacy && (active || !coordinated) {
            assert!(!allowed);
        }
        kani::cover!(allowed && !legacy, "abandoned owned drafts can recover");
        kani::cover!(allowed && legacy, "legacy drafts remain recoverable");
        kani::cover!(
            !allowed && active && !legacy,
            "a live sibling keeps its draft"
        );
        kani::cover!(
            !allowed && !coordinated && !legacy,
            "uncoordinated recovery requires a user action"
        );
    }

    #[kani::proof]
    #[kani::unwind(17)]
    fn browser_save_acknowledgements_preserve_later_edits() {
        let mut written = 0u8;
        let mut acknowledged = 0u8;
        let mut revision = false;
        let mut pending = None;
        let mut delayed_edit = false;
        for _ in 0..16 {
            let event: u8 = kani::any();
            match event % 3 {
                0 => written = kani::any(),
                1 if pending.is_none()
                    && may_save(written != acknowledged, false, false, false) =>
                {
                    pending = Some(written)
                }
                2 => {
                    if let Some(sent) = pending.take() {
                        delayed_edit |= sent != written;
                        acknowledged = sent;
                        revision = true;
                    }
                }
                _ => {}
            }
            let saved = is_saved(written == acknowledged, revision, pending.is_some());
            if saved {
                assert_eq!(written, acknowledged);
                assert!(pending.is_none());
            }
            if written != acknowledged {
                assert!(!saved);
            }
            kani::cover!(
                saved && delayed_edit,
                "a later save acknowledges edits made during a previous request"
            );
            kani::cover!(
                !saved && delayed_edit,
                "a delayed acknowledgement leaves newer edits dirty"
            );
        }
    }

    #[kani::proof]
    #[kani::unwind(17)]
    fn browser_refresh_replies_cannot_cross_navigation_or_composition() {
        let mut generation = 0u8;
        let mut revision = 0u8;
        let mut composing = false;
        let mut pending = None;
        let mut refused = false;
        let mut applied = false;
        for _ in 0..16 {
            let event: u8 = kani::any();
            match event % 5 {
                0 => generation += 1,
                1 => revision += 1,
                2 => composing = !composing,
                3 if pending.is_none() => pending = Some((generation, revision)),
                4 => {
                    if let Some((requested_generation, requested_revision)) = pending.take() {
                        let allowed = may_apply_remote(
                            true,
                            generation == requested_generation,
                            revision == requested_revision,
                            false,
                            false,
                            false,
                            composing,
                            false,
                        );
                        if allowed {
                            assert_eq!(generation, requested_generation);
                            assert_eq!(revision, requested_revision);
                            assert!(!composing);
                            applied = true;
                        } else {
                            refused = true;
                        }
                    }
                }
                _ => {}
            }
        }
        kani::cover!(
            applied && refused,
            "one trace both applies a current reply and refuses a stale reply"
        );
    }
}
