use tungsten_state::{ManifestChange, VaultPhase, manifest_change, private_access};

fn phase() -> VaultPhase {
    let value: u8 = kani::any();
    kani::assume(value < 3);
    match value {
        0 => VaultPhase::Pending,
        1 => VaultPhase::Migrating,
        _ => VaultPhase::Ready,
    }
}

fn rank(phase: VaultPhase) -> u8 {
    match phase {
        VaultPhase::Pending => 0,
        VaultPhase::Migrating => 1,
        VaultPhase::Ready => 2,
    }
}

#[kani::proof]
#[kani::unwind(2)]
fn manifest_initialization_cannot_skip_preparation() {
    let proposed = phase();
    let (same, identical): (bool, bool) = kani::any();
    let result = manifest_change(None, proposed, same, identical);
    assert_eq!(
        result == ManifestChange::Create,
        proposed != VaultPhase::Ready
    );
    kani::cover!(
        result == ManifestChange::Create,
        "pending or migrating vaults can be created"
    );
    kani::cover!(
        result == ManifestChange::Reject,
        "ready vaults cannot appear without preparation"
    );
}

#[kani::proof]
#[kani::unwind(2)]
fn manifest_updates_preserve_identity_and_advance_only() {
    let current = phase();
    let proposed = phase();
    let (same, identical): (bool, bool) = kani::any();
    let result = manifest_change(Some(current), proposed, same, identical);
    assert_ne!(result, ManifestChange::Create);
    match result {
        ManifestChange::Create => {}
        ManifestChange::Unchanged => {
            assert!(same && identical);
            assert_eq!(current, proposed);
        }
        ManifestChange::Replace => {
            assert!(same);
            assert!(rank(current) < rank(proposed));
        }
        ManifestChange::Reject => {
            assert!(
                !same || (rank(current) >= rank(proposed) && !(identical && current == proposed))
            );
        }
    }
    kani::cover!(
        result == ManifestChange::Replace,
        "same-vault transitions can advance"
    );
    kani::cover!(
        result == ManifestChange::Unchanged,
        "identical manifests are idempotent"
    );
    kani::cover!(
        result == ManifestChange::Reject,
        "foreign or regressing proposals are rejected"
    );
}

#[kani::proof]
#[kani::unwind(17)]
fn manifest_sequences_cannot_replace_a_vault_or_regress() {
    let mut current = None;
    let mut identity = 0u8;
    let mut changed = false;
    let mut refused = false;
    for _ in 0..16 {
        let proposed = phase();
        let proposed_identity: u8 = kani::any();
        let identical: bool = kani::any();
        let result = manifest_change(current, proposed, identity == proposed_identity, identical);
        let before = current;
        let before_identity = identity;
        match result {
            ManifestChange::Create => {
                assert!(current.is_none());
                assert_ne!(proposed, VaultPhase::Ready);
                current = Some(proposed);
                identity = proposed_identity;
                changed = true;
            }
            ManifestChange::Replace => {
                current = Some(proposed);
                changed = true;
            }
            ManifestChange::Unchanged | ManifestChange::Reject => {
                assert_eq!(current, before);
                refused |= result == ManifestChange::Reject;
            }
        }
        if let Some(before) = before {
            assert_eq!(identity, before_identity);
            assert!(rank(current.unwrap()) >= rank(before));
            if before == VaultPhase::Ready {
                assert_eq!(current, Some(VaultPhase::Ready));
            }
        }
    }
    kani::cover!(
        current == Some(VaultPhase::Ready) && changed && refused,
        "a vault becomes ready despite competing refused proposals"
    );
}

#[kani::proof]
#[kani::unwind(2)]
fn private_access_fails_closed_in_every_nonready_state() {
    let present: bool = kani::any();
    let phase = if present { Some(phase()) } else { None };
    let (session, router, failed): (bool, bool, bool) = kani::any();
    let allowed = private_access(phase, session, router, failed);
    assert_eq!(
        allowed,
        phase == Some(VaultPhase::Ready) && session && router && !failed
    );
    kani::cover!(allowed, "an enrolled device can use a ready vault");
    kani::cover!(
        !allowed && session && router,
        "a session and router cannot expose an unready or failed vault"
    );
    kani::cover!(
        !allowed && phase == Some(VaultPhase::Ready) && !session,
        "another unlocked device does not grant access"
    );
}
