//! MLS per Rondine (SPEC §7): un dispositivo, key package, chat 1:1, cifratura.
//!
//! La chiave di firma MLS è la chiave dell'identità e la credenziale `Basic` contiene la
//! stessa chiave pubblica: chi riceve controlla che coincidano, così il mittente di ogni
//! messaggio è autenticato dall'identità stessa.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::RwLock;

use openmls::prelude::tls_codec::{Deserialize as _, Serialize as _};
use openmls::prelude::*;
use openmls_basic_credential::SignatureKeyPair;
use openmls_memory_storage::MemoryStorage;
use openmls_rust_crypto::RustCrypto;
use rondine_core::{Identity, PublicKey};

pub const CIPHERSUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("MLS: {0}")]
    Mls(String),
    #[error("archivio MLS danneggiato")]
    Storage,
    #[error("gruppo sconosciuto")]
    UnknownGroup,
    #[error("il mittente non corrisponde alla sua credenziale")]
    BadSender,
    #[error("il key package non appartiene all'identità attesa")]
    WrongKeyPackage,
}

pub type Result<T> = std::result::Result<T, Error>;

fn mls<E: std::fmt::Debug>(e: E) -> Error {
    Error::Mls(format!("{e:?}"))
}

/// Provider openmls con archivio in memoria che si può salvare su disco.
#[derive(Default)]
pub struct Provider {
    crypto: RustCrypto,
    storage: MemoryStorage,
}

impl OpenMlsProvider for Provider {
    type CryptoProvider = RustCrypto;
    type RandProvider = RustCrypto;
    type StorageProvider = MemoryStorage;

    fn storage(&self) -> &Self::StorageProvider {
        &self.storage
    }

    fn crypto(&self) -> &Self::CryptoProvider {
        &self.crypto
    }

    fn rand(&self) -> &Self::RandProvider {
        &self.crypto
    }
}

impl Provider {
    pub fn export(&self) -> Result<Vec<u8>> {
        let values = self.storage.values.read().map_err(|_| Error::Storage)?;
        let mut out = Vec::new();
        write_u64(&mut out, values.len() as u64)?;
        for (k, v) in values.iter() {
            write_u64(&mut out, k.len() as u64)?;
            out.write_all(k).map_err(|_| Error::Storage)?;
            write_u64(&mut out, v.len() as u64)?;
            out.write_all(v).map_err(|_| Error::Storage)?;
        }
        Ok(out)
    }

    pub fn import(mut bytes: &[u8]) -> Result<Self> {
        let r = &mut bytes;
        let count = read_u64(r)?;
        let mut map = HashMap::new();
        for _ in 0..count {
            let k = read_vec(r)?;
            let v = read_vec(r)?;
            map.insert(k, v);
        }
        if !r.is_empty() {
            return Err(Error::Storage);
        }
        Ok(Self {
            crypto: RustCrypto::default(),
            storage: MemoryStorage {
                values: RwLock::new(map),
            },
        })
    }
}

fn write_u64(out: &mut Vec<u8>, n: u64) -> Result<()> {
    out.write_all(&n.to_be_bytes()).map_err(|_| Error::Storage)
}

fn read_u64(r: &mut &[u8]) -> Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b).map_err(|_| Error::Storage)?;
    Ok(u64::from_be_bytes(b))
}

fn read_vec(r: &mut &[u8]) -> Result<Vec<u8>> {
    let len = usize::try_from(read_u64(r)?).map_err(|_| Error::Storage)?;
    if len > r.len() {
        return Err(Error::Storage);
    }
    let mut v = vec![0u8; len];
    r.read_exact(&mut v).map_err(|_| Error::Storage)?;
    Ok(v)
}

/// Esito della decifratura di un messaggio ricevuto.
#[derive(Debug)]
pub enum Received {
    /// Messaggio applicativo, con il mittente autenticato.
    Application {
        group_id: Vec<u8>,
        sender: PublicKey,
        plaintext: Vec<u8>,
    },
    /// Messaggio di gestione del gruppo già applicato (commit, proposta).
    Control { group_id: Vec<u8> },
}

/// Gruppo appena raggiunto da un Welcome.
#[derive(Debug)]
pub struct Joined {
    pub group_id: Vec<u8>,
    pub members: Vec<PublicKey>,
}

/// Il dispositivo MLS di un'identità.
pub struct Device {
    provider: Provider,
    signer: SignatureKeyPair,
    credential: CredentialWithKey,
    identity: PublicKey,
}

impl Device {
    pub fn new(identity: &Identity) -> Self {
        Self::with_provider(identity, Provider::default())
    }

    /// Ricostruisce il dispositivo da un archivio salvato con [`Device::export`].
    pub fn restore(identity: &Identity, storage: &[u8]) -> Result<Self> {
        Ok(Self::with_provider(identity, Provider::import(storage)?))
    }

    fn with_provider(identity: &Identity, provider: Provider) -> Self {
        let public = identity.public();
        let signer = SignatureKeyPair::from_raw(
            SignatureScheme::ED25519,
            identity.seed().to_vec(),
            public.0.to_vec(),
        );
        let credential = CredentialWithKey {
            credential: BasicCredential::new(public.0.to_vec()).into(),
            signature_key: public.0.to_vec().into(),
        };
        Self {
            provider,
            signer,
            credential,
            identity: public,
        }
    }

    pub fn export(&self) -> Result<Vec<u8>> {
        self.provider.export()
    }

    pub fn identity(&self) -> PublicKey {
        self.identity
    }

    /// Crea `n` key package da pubblicare; l'ultimo è "di riserva" e si può riusare.
    pub fn key_packages(&self, n: usize) -> Result<Vec<Vec<u8>>> {
        (0..n)
            .map(|i| {
                let mut b = KeyPackage::builder().leaf_node_capabilities(Capabilities::new(
                    None,
                    None,
                    Some(&[ExtensionType::LastResort]),
                    None,
                    None,
                ));
                if i + 1 == n {
                    b = b.mark_as_last_resort();
                }
                let bundle = b
                    .build(
                        CIPHERSUITE,
                        &self.provider,
                        &self.signer,
                        self.credential.clone(),
                    )
                    .map_err(mls)?;
                bundle.key_package().tls_serialize_detached().map_err(mls)
            })
            .collect()
    }

    fn join_config() -> MlsGroupJoinConfig {
        MlsGroupJoinConfig::builder()
            .use_ratchet_tree_extension(true)
            .build()
    }

    /// Avvia una chat 1:1 con il proprietario di `key_package`, che deve essere `peer`.
    /// Restituisce l'id del gruppo e il Welcome da consegnare.
    pub fn start_dm(&self, key_package: &[u8], peer: &PublicKey) -> Result<(Vec<u8>, Vec<u8>)> {
        let kp = KeyPackageIn::tls_deserialize_exact(key_package)
            .map_err(mls)?
            .validate(self.provider.crypto(), ProtocolVersion::Mls10)
            .map_err(mls)?;
        let leaf = kp.leaf_node();
        if leaf.signature_key().as_slice() != peer.0
            || leaf.credential().serialized_content() != peer.0
        {
            return Err(Error::WrongKeyPackage);
        }
        let config = MlsGroupCreateConfig::builder()
            .ciphersuite(CIPHERSUITE)
            .use_ratchet_tree_extension(true)
            .build();
        let mut group = MlsGroup::new(
            &self.provider,
            &self.signer,
            &config,
            self.credential.clone(),
        )
        .map_err(mls)?;
        let (_commit, welcome, _info) = group
            .add_members(&self.provider, &self.signer, std::slice::from_ref(&kp))
            .map_err(mls)?;
        group.merge_pending_commit(&self.provider).map_err(mls)?;
        let welcome = welcome.to_bytes().map_err(mls)?;
        Ok((group.group_id().as_slice().to_vec(), welcome))
    }

    pub fn join(&self, welcome: &[u8]) -> Result<Joined> {
        let msg = MlsMessageIn::tls_deserialize_exact(welcome).map_err(mls)?;
        let MlsMessageBodyIn::Welcome(welcome) = msg.extract() else {
            return Err(Error::Mls("non è un Welcome".into()));
        };
        let group =
            StagedWelcome::new_from_welcome(&self.provider, &Self::join_config(), welcome, None)
                .map_err(mls)?
                .into_group(&self.provider)
                .map_err(mls)?;
        let members = group
            .members()
            .map(|m| member_identity(&m))
            .collect::<Result<Vec<_>>>()?;
        Ok(Joined {
            group_id: group.group_id().as_slice().to_vec(),
            members,
        })
    }

    fn load(&self, group_id: &[u8]) -> Result<MlsGroup> {
        MlsGroup::load(self.provider.storage(), &GroupId::from_slice(group_id))
            .map_err(mls)?
            .ok_or(Error::UnknownGroup)
    }

    pub fn encrypt(&self, group_id: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut group = self.load(group_id)?;
        group
            .create_message(&self.provider, &self.signer, plaintext)
            .map_err(mls)?
            .to_bytes()
            .map_err(mls)
    }

    pub fn decrypt(&self, message: &[u8]) -> Result<Received> {
        let msg = MlsMessageIn::tls_deserialize_exact(message).map_err(mls)?;
        let protocol = msg.try_into_protocol_message().map_err(mls)?;
        let group_id = protocol.group_id().as_slice().to_vec();
        let mut group = self.load(&group_id)?;
        let processed = group
            .process_message(&self.provider, protocol)
            .map_err(mls)?;
        let sender = match processed.sender() {
            Sender::Member(index) => group.member_at(*index).ok_or(Error::BadSender)?,
            _ => return Err(Error::BadSender),
        };
        let sender = member_identity(&sender)?;
        if processed.credential().serialized_content() != sender.0 {
            return Err(Error::BadSender);
        }
        match processed.into_content() {
            ProcessedMessageContent::ApplicationMessage(app) => Ok(Received::Application {
                group_id,
                sender,
                plaintext: app.into_bytes(),
            }),
            ProcessedMessageContent::StagedCommitMessage(commit) => {
                group
                    .merge_staged_commit(&self.provider, *commit)
                    .map_err(mls)?;
                Ok(Received::Control { group_id })
            }
            _ => Ok(Received::Control { group_id }),
        }
    }

    /// Identità dei membri di un gruppo.
    pub fn members(&self, group_id: &[u8]) -> Result<Vec<PublicKey>> {
        self.load(group_id)?
            .members()
            .map(|m| member_identity(&m))
            .collect()
    }
}

/// Identità di un membro, solo se credenziale e chiave di firma coincidono.
fn member_identity(m: &Member) -> Result<PublicKey> {
    let id = m.credential.serialized_content();
    if id != m.signature_key.as_slice() {
        return Err(Error::BadSender);
    }
    PublicKey::from_bytes(id).map_err(|_| Error::BadSender)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_uno_a_uno_con_salvataggio_su_disco() -> Result<()> {
        let alice_id = Identity::generate();
        let bob_id = Identity::generate();
        let alice = Device::new(&alice_id);
        let bob = Device::new(&bob_id);

        let kps = bob.key_packages(3)?;
        let bob = Device::restore(&bob_id, &bob.export()?)?;

        assert!(matches!(
            alice.start_dm(&kps[0], &alice_id.public()),
            Err(Error::WrongKeyPackage)
        ));
        let (gid, welcome) = alice.start_dm(&kps[0], &bob_id.public())?;
        let first = alice.encrypt(&gid, b"ciao Bob")?;

        let joined = bob.join(&welcome)?;
        assert_eq!(joined.group_id, gid);
        assert!(joined.members.contains(&alice_id.public()));

        let bob = Device::restore(&bob_id, &bob.export()?)?;
        match bob.decrypt(&first)? {
            Received::Application {
                sender, plaintext, ..
            } => {
                assert_eq!(sender, alice_id.public());
                assert_eq!(plaintext, b"ciao Bob");
            }
            other => panic!("atteso messaggio applicativo, ricevuto {other:?}"),
        }

        let reply = bob.encrypt(&gid, b"ciao Alice")?;
        let alice = Device::restore(&alice_id, &alice.export()?)?;
        match alice.decrypt(&reply)? {
            Received::Application {
                sender, plaintext, ..
            } => {
                assert_eq!(sender, bob_id.public());
                assert_eq!(plaintext, b"ciao Alice");
            }
            other => panic!("atteso messaggio applicativo, ricevuto {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn key_package_di_riserva_si_riusa() -> Result<()> {
        let bob_id = Identity::generate();
        let bob = Device::new(&bob_id);
        let kps = bob.key_packages(1)?;
        for _ in 0..2 {
            let alice = Device::new(&Identity::generate());
            let (_, welcome) = alice.start_dm(&kps[0], &bob_id.public())?;
            bob.join(&welcome)?;
        }
        Ok(())
    }
}
