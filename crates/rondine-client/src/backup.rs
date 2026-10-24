//! Backup e ripristino del profilo (SPEC §7.8).
//!
//! Il backup contiene identità, contatti, messaggi, gruppi e foto del profilo, cifrati con un
//! codice di 30 cifre. Non contiene lo stato MLS: dopo il ripristino le chat 1:1 si riaprono con
//! un `hello` di reset e si chiede agli amministratori di riaggiungerci ai gruppi.

use std::collections::BTreeMap;
use std::path::Path;

use reqwest::Method;
use rondine_core::envelope::AppMessage;
use rondine_core::username::{Handle, normalize_name};
use rondine_core::{Identity, b64, backup, now, password};
use rondine_mls::Device;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::state::State;
use crate::{
    Client, Error, KEYPACKAGE_BATCH, MLS_FILE, PHOTOS_DIR, Result, STATE_FILE, write_atomic,
};

#[derive(Serialize, Deserialize)]
struct Payload {
    v: u32,
    created_at: u64,
    state: State,
    /// Foto del profilo: "io" e le chiavi (b64) di chi l'ha mandata.
    #[serde(default)]
    photos: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct NamePassword {
    salt: String,
}

impl Client {
    /// Il JSON del backup: stato e foto del profilo (SPEC §7.8).
    fn backup_payload(&self) -> Result<Vec<u8>> {
        let mut photos = BTreeMap::new();
        if let Ok(dir) = std::fs::read_dir(self.dir.join(PHOTOS_DIR)) {
            for entry in dir.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(who) = name.strip_suffix(".jpg")
                    && let Ok(bytes) = std::fs::read(entry.path())
                {
                    photos.insert(who.to_string(), b64::encode(bytes));
                }
            }
        }
        let payload = Payload {
            v: 1,
            created_at: now(),
            state: self.state.clone(),
            photos,
        };
        Ok(serde_json::to_vec(&payload)?)
    }

    /// Il backup cifrato e il codice per aprirlo (da mostrare una volta e far annotare).
    pub fn export_backup(&self) -> Result<(Vec<u8>, String)> {
        let code = backup::new_code();
        let file = backup::seal(&code, &self.backup_payload()?)?;
        Ok((file, code))
    }

    /// Il backup cifrato con la password (versione 2, SPEC §7.8): serve averla scelta.
    pub fn export_backup_with_password(&self) -> Result<Vec<u8>> {
        let keys = self
            .state
            .password
            .as_ref()
            .ok_or_else(|| Error::Invalid("prima scegli una password".into()))?;
        Ok(backup::seal_with_keys(keys, &self.backup_payload()?)?)
    }

    pub fn has_password(&self) -> bool {
        self.state.password.is_some()
    }

    /// Sceglie (o cambia) la password (SPEC §3.4): protegge il nome utente, se c'è, e cifra i
    /// backup automatici.
    pub async fn set_password(&mut self, pw: &str) -> Result<()> {
        let pw = pw.to_string();
        let keys = tokio::task::spawn_blocking(move || password::new_keys(&pw))
            .await
            .map_err(|e| Error::Invalid(format!("password non salvata: {e}")))??;
        let old = self.state.password.replace(keys);
        if self.state.username.is_some()
            && self.has_node()
            && let Err(e) = self.upload_name_password().await
        {
            self.state.password = old;
            return Err(e);
        }
        self.save()
    }

    /// Manda al nodo del nome il sale e la verifica della password.
    pub(crate) async fn upload_name_password(&mut self) -> Result<()> {
        let (Some(keys), Some(handle)) = (&self.state.password, &self.state.username) else {
            return Ok(());
        };
        let name = Handle::parse(handle)?.name;
        let body = json!({ "salt": keys.salt, "verifier": keys.verifier()? });
        self.net
            .owner(
                &self.identity,
                Method::PUT,
                &self.state.node,
                &format!("/v1/names/{name}/password"),
                Some(&body),
            )
            .await?;
        Ok(())
    }

    /// La prova della password per il nome `name` del mio nodo (SPEC §3.4).
    async fn name_auth(&self, name: &str, pw: &str) -> Result<String> {
        self.require_node()?;
        let name = normalize_name(name.trim().trim_start_matches('@'))?;
        let found: NamePassword = self
            .net
            .get(&self.state.node, &format!("/v1/names/{name}/password"))
            .await?;
        let salt = b64::decode(&found.salt)?;
        let pw = pw.to_string();
        let keys = tokio::task::spawn_blocking(move || password::keys(&pw, &salt))
            .await
            .map_err(|e| Error::Invalid(format!("password non controllata: {e}")))??;
        Ok(keys.auth)
    }

    /// Riprende un nome utente protetto da password (per esempio dopo aver perso il telefono
    /// senza backup). Se il nome aveva un numero verificato, va verificato prima lo stesso numero
    /// (con [`Client::confirm_phone_with_password`]). Poi il nome resta protetto dalla stessa
    /// password.
    pub async fn recover_username(&mut self, name: &str, pw: &str) -> Result<String> {
        let auth = self.name_auth(name, pw).await?;
        let plain = name.trim().trim_start_matches('@').to_string();
        let handle = self.claim_username_with(&plain, Some(&auth)).await?;
        self.set_password(pw).await?;
        Ok(handle)
    }

    /// Conferma il numero anche se è protetto dalla password di chi lo aveva (SPEC §13.2): `name`
    /// è il suo nome utente su questo nodo.
    pub async fn confirm_phone_with_password(
        &mut self,
        code: &str,
        name: &str,
        pw: &str,
    ) -> Result<()> {
        let auth = self.name_auth(name, pw).await?;
        self.confirm_phone_with(code, Some(&auth)).await
    }

    /// Ripristina un backup in una cartella vuota. La rete serve solo dopo: alla prima
    /// sincronizzazione il client riapre le chat e chiede di rientrare nei gruppi.
    /// `secret` è il codice di 30 cifre (versione 1) o la password (versione 2).
    pub async fn restore_backup(dir: &Path, file: &[u8], secret: &str) -> Result<Client> {
        if Client::exists(dir) {
            return Err(Error::Invalid(
                "in questa cartella c'è già un profilo".into(),
            ));
        }
        let (plain, keys) = {
            let (file, secret) = (file.to_vec(), secret.to_string());
            tokio::task::spawn_blocking(move || {
                if backup::needs_password(&file) {
                    backup::open_with_password(&secret, &file).map(|(p, k)| (p, Some(k)))
                } else {
                    backup::open(&secret, &file).map(|p| (p, None))
                }
            })
            .await
            .map_err(|e| Error::Invalid(format!("ripristino interrotto: {e}")))??
        };
        let mut client = Self::restore_payload(dir, &plain).await?;
        // Stessa password: i backup automatici continuano senza chiederla.
        if keys.is_some() {
            client.state.password = keys;
            client.save()?;
        }
        Ok(client)
    }

    /// Il file di backup si apre con la password (versione 2) e non con il codice?
    pub fn is_password_backup(file: &[u8]) -> bool {
        backup::needs_password(file)
    }

    /// Profilo dal backup decifrato (SPEC §7.8).
    async fn restore_payload(dir: &Path, plain: &[u8]) -> Result<Client> {
        let payload: Payload = serde_json::from_slice(plain)?;
        rondine_core::check_version(payload.v)?;
        let mut state = payload.state;
        let identity = Identity::from_seed(b64::decode_array::<32>(&state.seed)?);
        // Niente dello stato MLS vecchio vale più: chat da riaprire, gruppi da cui rientrare.
        for c in &mut state.contacts {
            c.groups.clear();
            c.addrs.clear();
        }
        state.rejoin_groups = state
            .group_chats
            .iter()
            .filter(|g| g.active)
            .map(|g| g.id.clone())
            .collect();
        for g in &mut state.group_chats {
            g.active = false;
            g.my_permit = None;
        }
        state.outbox.clear();
        // Foto e documenti delle chat non sono nel backup.
        for m in &mut state.messages {
            if let Some(a) = &mut m.attachment {
                a.path = None;
                a.at = None;
            }
        }
        for m in &mut state.group_messages {
            if let Some(a) = &mut m.attachment {
                a.path = None;
                a.at = None;
            }
        }
        state.pending_restore = true;
        std::fs::create_dir_all(dir)?;
        let photos = dir.join(PHOTOS_DIR);
        for (who, b) in &payload.photos {
            if let Ok(bytes) = b64::decode(b) {
                std::fs::create_dir_all(&photos)?;
                write_atomic(
                    &photos.join(format!("{}.jpg", rondine_core::file::safe_name(who))),
                    &bytes,
                )?;
            }
        }
        let device = Device::new(&identity);
        write_atomic(&dir.join(STATE_FILE), &serde_json::to_vec_pretty(&state)?)?;
        write_atomic(&dir.join(MLS_FILE), &device.export()?)?;
        Client::open(dir)
    }

    /// Dopo un ripristino (SPEC §7.8): key package nuovi al posto dei vecchi, chat 1:1 riaperte
    /// con `reset`, richiesta di rientro agli amministratori dei gruppi. Si ritenta alla
    /// sincronizzazione successiva finché i key package non sono sul nodo.
    pub(crate) async fn finish_restore(&mut self) {
        if !self.state.pending_restore {
            return;
        }
        if self.has_node() {
            let Ok(kps) = self.device.key_packages(KEYPACKAGE_BATCH) else {
                return;
            };
            let items: Vec<String> = kps.iter().map(b64::encode).collect();
            let path = format!("/v1/inboxes/{}/keypackages", self.state.inbox);
            let res = self
                .net
                .owner(
                    &self.identity,
                    Method::POST,
                    &self.state.node,
                    &path,
                    Some(&json!({ "items": items, "replace": true })),
                )
                .await;
            if res.is_err() {
                return;
            }
        }
        self.state.pending_restore = false;
        let _ = self.save();
        let contacts: Vec<_> = self
            .state
            .contacts
            .iter()
            .filter(|c| !c.blocked)
            .map(|c| c.identity)
            .collect();
        for id in contacts {
            // Un contatto irraggiungibile adesso riceverà il saluto quando gli scriverò.
            let _ = self.start_chat_with(&id, true).await;
        }
        let me = self.identity.public();
        for gid in std::mem::take(&mut self.state.rejoin_groups) {
            let Some(admin) = self
                .state
                .group_chats
                .iter()
                .find(|g| g.id == gid)
                .map(|g| g.admin)
            else {
                continue;
            };
            if admin == me {
                continue;
            }
            let Some(contact) = self
                .state
                .contacts
                .iter()
                .find(|c| c.identity == admin && !c.groups.is_empty())
                .cloned()
            else {
                continue;
            };
            let _ = self
                .send_app(&contact, &AppMessage::Rejoin { v: 1, group: gid })
                .await;
        }
        let _ = self.save();
    }
}
