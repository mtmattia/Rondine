//! MLS per Rondine (SPEC §7): un dispositivo, key package, chat 1:1, gruppi, cifratura.
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
/// Epoche passate di cui si tengono le chiavi, per i messaggi che incrociano un commit (SPEC §7.2).
pub const MAX_PAST_EPOCHS: usize = 5;

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
    #[error("commit da un membro che non può modificare il gruppo")]
    CommitNotAllowed,
    #[error("membro sconosciuto")]
    UnknownMember,
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
    Control {
        group_id: Vec<u8>,
        sender: PublicKey,
    },
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
            .wire_format_policy(PURE_CIPHERTEXT_WIRE_FORMAT_POLICY)
            .max_past_epochs(MAX_PAST_EPOCHS)
            .build()
    }

    /// Controlla che un key package sia valido e appartenga a `owner`.
    fn validated(&self, key_package: &[u8], owner: &PublicKey) -> Result<KeyPackage> {
        let kp = KeyPackageIn::tls_deserialize_exact(key_package)
            .map_err(mls)?
            .validate(self.provider.crypto(), ProtocolVersion::Mls10)
            .map_err(mls)?;
        let leaf = kp.leaf_node();
        if leaf.signature_key().as_slice() != owner.0
            || leaf.credential().serialized_content() != owner.0
        {
            return Err(Error::WrongKeyPackage);
        }
        Ok(kp)
    }

    fn validated_all(&self, key_packages: &[(Vec<u8>, PublicKey)]) -> Result<Vec<KeyPackage>> {
        key_packages
            .iter()
            .map(|(kp, owner)| self.validated(kp, owner))
            .collect()
    }

    /// Avvia una chat 1:1 con il proprietario di `key_package`, che deve essere `peer`.
    /// Restituisce l'id del gruppo e il Welcome da consegnare.
    pub fn start_dm(&self, key_package: &[u8], peer: &PublicKey) -> Result<(Vec<u8>, Vec<u8>)> {
        self.create_group(&[(key_package.to_vec(), *peer)])
    }

    /// Crea un gruppo con i proprietari dei key package (coppie key package, identità attesa).
    /// Restituisce l'id del gruppo e il Welcome da consegnare a tutti.
    pub fn create_group(
        &self,
        key_packages: &[(Vec<u8>, PublicKey)],
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        let kps = self.validated_all(key_packages)?;
        let config = MlsGroupCreateConfig::builder()
            .ciphersuite(CIPHERSUITE)
            .use_ratchet_tree_extension(true)
            .wire_format_policy(PURE_CIPHERTEXT_WIRE_FORMAT_POLICY)
            .max_past_epochs(MAX_PAST_EPOCHS)
            .build();
        let mut group = MlsGroup::new(
            &self.provider,
            &self.signer,
            &config,
            self.credential.clone(),
        )
        .map_err(mls)?;
        let (_commit, welcome, _info) = group
            .add_members(&self.provider, &self.signer, &kps)
            .map_err(mls)?;
        group.merge_pending_commit(&self.provider).map_err(mls)?;
        let welcome = welcome.to_bytes().map_err(mls)?;
        Ok((group.group_id().as_slice().to_vec(), welcome))
    }

    /// Aggiunge membri: restituisce il commit per i membri attuali e il Welcome per i nuovi.
    pub fn add_members(
        &self,
        group_id: &[u8],
        key_packages: &[(Vec<u8>, PublicKey)],
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        let kps = self.validated_all(key_packages)?;
        let mut group = self.load(group_id)?;
        let (commit, welcome, _info) = group
            .add_members(&self.provider, &self.signer, &kps)
            .map_err(mls)?;
        group.merge_pending_commit(&self.provider).map_err(mls)?;
        Ok((
            commit.to_bytes().map_err(mls)?,
            welcome.to_bytes().map_err(mls)?,
        ))
    }

    /// Toglie membri: restituisce il commit da consegnare a tutti, tolti compresi.
    pub fn remove_members(&self, group_id: &[u8], who: &[PublicKey]) -> Result<Vec<u8>> {
        let mut group = self.load(group_id)?;
        let mut leaves = Vec::new();
        for id in who {
            let m = group
                .members()
                .find(|m| m.credential.serialized_content() == id.0)
                .ok_or(Error::UnknownMember)?;
            leaves.push(m.index);
        }
        let (commit, _welcome, _info) = group
            .remove_members(&self.provider, &self.signer, &leaves)
            .map_err(mls)?;
        group.merge_pending_commit(&self.provider).map_err(mls)?;
        commit.to_bytes().map_err(mls)
    }

    /// Vero finché si fa parte del gruppo (falso dopo essere stati tolti).
    pub fn is_member(&self, group_id: &[u8]) -> Result<bool> {
        Ok(self.load(group_id)?.is_active())
    }

    pub fn join(&self, welcome: &[u8]) -> Result<Joined> {
        let msg = MlsMessageIn::tls_deserialize_exact(welcome).map_err(mls)?;
        let MlsMessageBodyIn::Welcome(welcome) = msg.extract() else {
            return Err(Error::Mls("non è un Welcome".into()));
        };
        let builder =
            StagedWelcome::build_from_welcome(&self.provider, &Self::join_config(), welcome)
                .map_err(mls)?;
        let gid = builder
            .processed_welcome()
            .unverified_group_info()
            .group_id()
            .clone();
        // Riaggiunto a un gruppo da cui ero stato tolto: il vecchio stato, non più attivo, lascia
        // il posto a quello nuovo. Un gruppo ancora attivo invece non si sostituisce mai.
        let builder = match MlsGroup::load(self.provider.storage(), &gid).map_err(mls)? {
            Some(old) if !old.is_active() => builder.replace_old_group(),
            _ => builder,
        };
        let group = builder
            .build()
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

    /// Decifra un messaggio. Un commit si applica solo se `allow_commit(id gruppo, autore)`.
    pub fn decrypt(
        &self,
        message: &[u8],
        allow_commit: impl Fn(&[u8], &PublicKey) -> bool,
    ) -> Result<Received> {
        let msg = MlsMessageIn::tls_deserialize_exact(message).map_err(mls)?;
        let protocol = msg.try_into_protocol_message().map_err(mls)?;
        let group_id = protocol.group_id().as_slice().to_vec();
        let mut group = self.load(&group_id)?;
        let processed = group
            .process_message(&self.provider, protocol)
            .map_err(mls)?;
        if !matches!(processed.sender(), Sender::Member(_)) {
            return Err(Error::BadSender);
        }
        // Ogni foglia entrata nel gruppo ha credenziale = chiave di firma (controllo in `join`,
        // `validated` e qui sotto sui commit), e openmls ha verificato la firma con la chiave
        // della foglia nell'epoca del messaggio: la credenziale identifica il mittente anche se
        // nel frattempo è stato tolto.
        let sender = PublicKey::from_bytes(processed.credential().serialized_content())
            .map_err(|_| Error::BadSender)?;
        match processed.into_content() {
            ProcessedMessageContent::ApplicationMessage(app) => Ok(Received::Application {
                group_id,
                sender,
                plaintext: app.into_bytes(),
            }),
            ProcessedMessageContent::StagedCommitMessage(commit) => {
                if !allow_commit(&group_id, &sender) {
                    return Err(Error::CommitNotAllowed);
                }
                check_commit_leaves(&commit)?;
                group
                    .merge_staged_commit(&self.provider, *commit)
                    .map_err(mls)?;
                Ok(Received::Control { group_id, sender })
            }
            // Le proposte da sole non si accettano: cambia il gruppo solo un commit permesso.
            _ => Err(Error::CommitNotAllowed),
        }
    }

    /// Segreto esportato dall'epoca attuale del gruppo (MLS-Exporter, RFC 9420 §8.5): lo
    /// conoscono solo i membri.
    pub fn export_secret(&self, group_id: &[u8], label: &str) -> Result<[u8; 32]> {
        let secret = self
            .load(group_id)?
            .export_secret(self.provider.crypto(), label, &[], 32)
            .map_err(mls)?;
        secret
            .try_into()
            .map_err(|_| Error::Mls("segreto esportato corto".into()))
    }

    /// Identità dei membri di un gruppo.
    pub fn members(&self, group_id: &[u8]) -> Result<Vec<PublicKey>> {
        self.load(group_id)?
            .members()
            .map(|m| member_identity(&m))
            .collect()
    }
}

/// Una foglia è accettabile solo se la credenziale contiene la sua stessa chiave di firma.
fn check_leaf(leaf: &LeafNode) -> Result<()> {
    if leaf.credential().serialized_content() != leaf.signature_key().as_slice() {
        return Err(Error::BadSender);
    }
    Ok(())
}

/// Controlla tutte le foglie nuove o cambiate da un commit, prima di applicarlo.
fn check_commit_leaves(commit: &StagedCommit) -> Result<()> {
    for add in commit.add_proposals() {
        check_leaf(add.add_proposal().key_package().leaf_node())?;
    }
    for update in commit.update_proposals() {
        check_leaf(update.update_proposal().leaf_node())?;
    }
    if let Some(leaf) = commit.update_path_leaf_node() {
        check_leaf(leaf)?;
    }
    Ok(())
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
    fn segreto_esportato_uguale_per_i_due_membri() -> Result<()> {
        let (alice_id, bob_id) = (Identity::generate(), Identity::generate());
        let (alice, bob) = (Device::new(&alice_id), Device::new(&bob_id));
        let kps = bob.key_packages(1)?;
        let (gid, welcome) = alice.start_dm(&kps[0], &bob_id.public())?;
        bob.join(&welcome)?;
        let a = alice.export_secret(&gid, "rondine/v1/trasloco")?;
        assert_eq!(a, bob.export_secret(&gid, "rondine/v1/trasloco")?);
        assert_ne!(a, alice.export_secret(&gid, "altro")?);
        Ok(())
    }

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
        match bob.decrypt(&first, |_, _| false)? {
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
        match alice.decrypt(&reply, |_, _| false)? {
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

    fn app(r: Received) -> Result<(PublicKey, Vec<u8>)> {
        match r {
            Received::Application {
                sender, plaintext, ..
            } => Ok((sender, plaintext)),
            other => Err(Error::Mls(format!(
                "atteso messaggio applicativo: {other:?}"
            ))),
        }
    }

    #[test]
    fn gruppo_aggiunte_rimozioni_e_solo_admin() -> Result<()> {
        let ids: Vec<Identity> = (0..4).map(|_| Identity::generate()).collect();
        let devs: Vec<Device> = ids.iter().map(Device::new).collect();
        let (a, b, c, d) = (&devs[0], &devs[1], &devs[2], &devs[3]);
        let pk = |i: usize| ids[i].public();
        let kp = |i: usize| -> Result<(Vec<u8>, PublicKey)> {
            Ok((devs[i].key_packages(1)?.remove(0), pk(i)))
        };
        let admin = pk(0);
        let only_admin = |_: &[u8], s: &PublicKey| *s == admin;

        // A crea il gruppo con B e C.
        let (gid, welcome) = a.create_group(&[kp(1)?, kp(2)?])?;
        for dev in [b, c] {
            let j = dev.join(&welcome)?;
            assert_eq!(j.group_id, gid);
            assert_eq!(j.members.len(), 3);
        }

        // B scrive: lo leggono A e C.
        let m = b.encrypt(&gid, b"ciao a tutti")?;
        assert_eq!(
            app(a.decrypt(&m, only_admin)?)?,
            (pk(1), b"ciao a tutti".to_vec())
        );
        assert_eq!(
            app(c.decrypt(&m, only_admin)?)?,
            (pk(1), b"ciao a tutti".to_vec())
        );

        // C scrive prima di vedere l'aggiunta di D: A lo legge lo stesso (epoca passata).
        let late = c.encrypt(&gid, b"sono in ritardo")?;
        let (commit, welcome) = a.add_members(&gid, &[kp(3)?])?;
        assert_eq!(app(a.decrypt(&late, only_admin)?)?.1, b"sono in ritardo");
        for dev in [b, c] {
            assert!(matches!(
                dev.decrypt(&commit, only_admin)?,
                Received::Control { .. }
            ));
        }
        d.join(&welcome)?;
        assert_eq!(a.members(&gid)?.len(), 4);
        let m = d.encrypt(&gid, b"eccomi")?;
        assert_eq!(app(b.decrypt(&m, only_admin)?)?.1, b"eccomi");

        // Un commit di B (non admin) viene scartato senza cambiare il gruppo.
        let rogue = b.remove_members(&gid, &[pk(2)])?;
        assert!(matches!(
            c.decrypt(&rogue, only_admin),
            Err(Error::CommitNotAllowed)
        ));
        assert_eq!(c.members(&gid)?.len(), 4);

        // C scrive e poi A lo toglie: il messaggio, arrivato dopo, resta leggibile e di C.
        let before_removal = c.encrypt(&gid, b"ultimo messaggio")?;
        // A toglie C: gli altri lo vedono sparire, C sa di essere fuori.
        let commit = a.remove_members(&gid, &[pk(2)])?;
        assert_eq!(
            app(a.decrypt(&before_removal, only_admin)?)?,
            (pk(2), b"ultimo messaggio".to_vec())
        );
        d.decrypt(&commit, only_admin)?;
        c.decrypt(&commit, only_admin)?;
        assert!(!d.members(&gid)?.contains(&pk(2)));
        assert!(!c.is_member(&gid)?);
        assert!(d.is_member(&gid)?);
        let m = a.encrypt(&gid, b"senza C")?;
        assert_eq!(app(d.decrypt(&m, only_admin)?)?.1, b"senza C");
        assert!(c.decrypt(&m, only_admin).is_err());
        Ok(())
    }

    #[test]
    fn commit_con_credenziale_falsa_respinto() -> Result<()> {
        let (a_id, b_id, mallory_id, victim) = (
            Identity::generate(),
            Identity::generate(),
            Identity::generate(),
            Identity::generate().public(),
        );
        let (a, b) = (Device::new(&a_id), Device::new(&b_id));
        let (gid, welcome) = a.create_group(&[(b.key_packages(1)?.remove(0), b_id.public())])?;
        b.join(&welcome)?;

        // A, senza passare dai controlli, aggiunge una foglia che dice di essere `victim`
        // ma firma con la chiave di Mallory.
        let mallory = Device::new(&mallory_id);
        let forged = KeyPackage::builder()
            .build(
                CIPHERSUITE,
                &mallory.provider,
                &mallory.signer,
                CredentialWithKey {
                    credential: BasicCredential::new(victim.0.to_vec()).into(),
                    signature_key: mallory_id.public().0.to_vec().into(),
                },
            )
            .map_err(mls)?;
        let mut group = a.load(&gid)?;
        let (commit, _, _) = group
            .add_members(&a.provider, &a.signer, &[forged.key_package().clone()])
            .map_err(mls)?;
        let commit = commit.to_bytes().map_err(mls)?;

        let admin = a_id.public();
        assert!(matches!(
            b.decrypt(&commit, |_, s| *s == admin),
            Err(Error::BadSender)
        ));
        assert_eq!(b.members(&gid)?.len(), 2);
        Ok(())
    }
}
