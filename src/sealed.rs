use crate::Error;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hkdf::Hkdf;
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
pub use tungsten_state::VaultPhase;
use zeroize::Zeroizing;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VaultManifest {
    pub format: u32,
    pub id: String,
    pub key: String,
    pub root: String,
    pub check: String,
}

impl VaultManifest {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        let valid_key = |text: &str, length: usize| {
            URL_SAFE_NO_PAD
                .decode(text)
                .is_ok_and(|bytes| bytes.len() == length && bytes.starts_with(&[84, 86, 76, 84, 1]))
        };
        if self.format != 1
            || !crate::rules::valid_id(self.id.as_bytes())
            || !crate::rules::valid_id(self.key.as_bytes())
            || !valid_key(&self.root, 81)
            || !valid_key(&self.check, 67)
        {
            return Err(Error::InvalidData("The vault manifest is invalid."));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeManifest {
    pub format: u32,
    #[serde(with = "PhaseEncoding")]
    pub phase: VaultPhase,
    pub vault: VaultManifest,
    pub check: String,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "VaultPhase", rename_all = "snake_case")]
enum PhaseEncoding {
    Pending,
    Migrating,
    Ready,
}

impl RuntimeManifest {
    fn message(&self) -> String {
        format!(
            "tungsten.server-check.1:{}:{}:{}:{}:{}",
            self.vault.id,
            self.vault.key,
            self.vault.root,
            self.vault.check,
            match self.phase {
                VaultPhase::Pending => "pending",
                VaultPhase::Migrating => "migrating",
                VaultPhase::Ready => "ready",
            }
        )
    }

    fn verification(&self, seed: &[u8; 32]) -> Result<Hmac<Sha256>, Error> {
        let mut key = Zeroizing::new([0; 32]);
        Hkdf::<Sha256>::new(Some(self.vault.id.as_bytes()), seed)
            .expand(b"tungsten.runtime-verification.1", &mut *key)
            .map_err(|_| Error::Worker)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&*key).map_err(|_| Error::Worker)?;
        mac.update(self.message().as_bytes());
        Ok(mac)
    }

    pub fn create(vault: VaultManifest, seed: &[u8; 32]) -> Result<Self, Error> {
        vault.validate()?;
        let mut manifest = Self {
            format: 1,
            phase: VaultPhase::Pending,
            vault,
            check: String::new(),
        };
        manifest.check =
            URL_SAFE_NO_PAD.encode(manifest.verification(seed)?.finalize().into_bytes());
        Ok(manifest)
    }

    pub fn ready(&self, seed: &[u8; 32]) -> Result<Self, Error> {
        self.transition(seed, VaultPhase::Ready)
    }

    pub fn migrating(&self, seed: &[u8; 32]) -> Result<Self, Error> {
        self.transition(seed, VaultPhase::Migrating)
    }

    fn transition(&self, seed: &[u8; 32], phase: VaultPhase) -> Result<Self, Error> {
        let _key = self.database_key(seed)?;
        let mut manifest = self.clone();
        manifest.phase = phase;
        manifest.check =
            URL_SAFE_NO_PAD.encode(manifest.verification(seed)?.finalize().into_bytes());
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), Error> {
        self.vault.validate()?;
        if self.format != 1
            || !URL_SAFE_NO_PAD
                .decode(&self.check)
                .is_ok_and(|bytes| bytes.len() == 32)
        {
            return Err(Error::InvalidData("The server vault manifest is invalid."));
        }
        Ok(())
    }

    pub fn database_key(&self, seed: &[u8; 32]) -> Result<Zeroizing<[u8; 32]>, Error> {
        self.validate()?;
        let check = URL_SAFE_NO_PAD
            .decode(&self.check)
            .map_err(|_| Error::VaultKey)?;
        self.verification(seed)?
            .verify_slice(&check)
            .map_err(|_| Error::VaultKey)?;
        let mut key = Zeroizing::new([0; 32]);
        Hkdf::<Sha256>::new(Some(self.vault.id.as_bytes()), seed)
            .expand(b"tungsten.database-key.1", &mut *key)
            .map_err(|_| Error::Worker)?;
        Ok(key)
    }
}
