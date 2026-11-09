#![no_std]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VaultPhase {
    Pending,
    Migrating,
    Ready,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestChange {
    Create,
    Unchanged,
    Replace,
    Reject,
}

pub fn manifest_change(
    current: Option<VaultPhase>,
    proposed: VaultPhase,
    same_vault: bool,
    identical: bool,
) -> ManifestChange {
    let Some(current) = current else {
        return if proposed == VaultPhase::Ready {
            ManifestChange::Reject
        } else {
            ManifestChange::Create
        };
    };
    if !same_vault {
        return ManifestChange::Reject;
    }
    if identical && current == proposed {
        return ManifestChange::Unchanged;
    }
    match (current, proposed) {
        (VaultPhase::Pending, VaultPhase::Migrating | VaultPhase::Ready)
        | (VaultPhase::Migrating, VaultPhase::Ready) => ManifestChange::Replace,
        _ => ManifestChange::Reject,
    }
}

pub fn private_access(
    phase: Option<VaultPhase>,
    valid_session: bool,
    router_ready: bool,
    failed: bool,
) -> bool {
    phase == Some(VaultPhase::Ready) && valid_session && router_ready && !failed
}
