use tungsten_state::{ManifestChange, VaultPhase, manifest_change, private_access};

#[test]
fn manifest_changes_require_a_stable_identity_and_forward_progress() {
    use ManifestChange::{Create, Reject, Replace, Unchanged};
    use VaultPhase::{Migrating, Pending, Ready};
    for (current, proposed, same_vault, identical, expected) in [
        (None, Pending, false, false, Create),
        (None, Migrating, false, false, Create),
        (None, Ready, true, true, Reject),
        (Some(Pending), Pending, true, true, Unchanged),
        (Some(Migrating), Migrating, true, true, Unchanged),
        (Some(Ready), Ready, true, true, Unchanged),
        (Some(Pending), Migrating, true, false, Replace),
        (Some(Pending), Ready, true, false, Replace),
        (Some(Migrating), Ready, true, false, Replace),
        (Some(Ready), Migrating, true, false, Reject),
        (Some(Migrating), Pending, true, false, Reject),
        (Some(Ready), Ready, true, false, Reject),
        (Some(Pending), Ready, false, false, Reject),
        (Some(Ready), Ready, false, true, Reject),
        (Some(Ready), Pending, true, true, Reject),
    ] {
        assert_eq!(
            manifest_change(current, proposed, same_vault, identical),
            expected
        );
    }
}

#[test]
fn private_requests_require_a_ready_vault_and_a_valid_device_session() {
    for phase in [
        None,
        Some(VaultPhase::Pending),
        Some(VaultPhase::Migrating),
        Some(VaultPhase::Ready),
    ] {
        for mask in 0..8 {
            assert_eq!(
                private_access(phase, mask & 1 != 0, mask & 2 != 0, mask & 4 != 0),
                phase == Some(VaultPhase::Ready) && mask == 3,
            );
        }
    }
}
