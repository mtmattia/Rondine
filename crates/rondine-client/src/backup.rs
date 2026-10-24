//! Backup e ripristino del profilo (SPEC §7.8).
//!
//! Il backup contiene identità, contatti, messaggi, gruppi e foto del profilo, cifrati con un
//! codice di 30 cifre. Non contiene lo stato MLS: dopo il ripristino le chat 1:1 si riaprono con
//! un `hello` di reset e si chiede agli amministratori di riaggiungerci ai gruppi.

use std::collections::BTreeMap;
use std::path::Path;

use reqwest::Method;
use rondine_core::envelope::AppMessage;
use rondine_core::{Identity, b64, backup, now, phone, vault};
use rondine_mls::Device;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::net::{Net, parse};
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

/// Ogni quanto si rifà la cassaforte sul nodo (SPEC §7.8).
pub const VAULT_EVERY_SECS: u64 = 6 * 3600;

/// Di chi è la cassaforte da aprire: nome utente (`mattia`, `@mattia`, `mattia@nodo`) o numero.
fn vault_who(who: &str) -> Result<serde_json::Value> {
    let w = who.trim();
    if w.starts_with('+') || w.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        let n = phone::normalize(w, phone::DEFAULT_COUNTRY)?;
        return Ok(json!({ "phone_hash": phone::phone_hash(&n)? }));
    }
    let name = w.trim_start_matches('@');
    let name = name.split('@').next().unwrap_or(name);
    Ok(json!({ "name": name }))
}

#[derive(Deserialize)]
struct VaultSalt {
    salt: String,
}

#[derive(Deserialize)]
struct VaultOpened {
    blob: String,
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

    /// C'è la cassaforte sul nodo, e quando è stata copiata l'ultima volta (0 se mai).
    pub fn vault_status(&self) -> (bool, u64) {
        (self.state.vault.is_some(), self.state.vault_at)
    }

    /// Ogni quanto rifare la cassaforte (per i test; di solito 6 ore).
    pub fn set_vault_every(&mut self, secs: u64) {
        self.vault_every = secs;
    }

    /// Sceglie (o cambia) la password e copia subito il profilo nella cassaforte del nodo.
    pub async fn set_vault_password(&mut self, password: &str) -> Result<()> {
        if !self.has_node() {
            return Err(Error::Invalid(
                "la cassaforte sta sul nodo: scegline uno".into(),
            ));
        }
        let pw = password.to_string();
        let keys = tokio::task::spawn_blocking(move || vault::new_keys(&pw))
            .await
            .map_err(|e| Error::Invalid(format!("password non salvata: {e}")))??;
        let old = self.state.vault.replace(keys);
        if let Err(e) = self.backup_to_vault().await {
            self.state.vault = old;
            return Err(e);
        }
        Ok(())
    }

    /// Copia adesso il profilo nella cassaforte.
    pub async fn backup_to_vault(&mut self) -> Result<()> {
        let keys = self
            .state
            .vault
            .clone()
            .ok_or_else(|| Error::Invalid("nessuna password per la cassaforte".into()))?;
        let blob = vault::seal(&keys, &self.backup_payload()?)?;
        let body = json!({
            "salt": keys.salt,
            "verifier": keys.verifier()?,
            "blob": b64::encode(&blob),
        });
        let path = format!("/v1/inboxes/{}/vault", self.state.inbox);
        self.net
            .owner(
                &self.identity,
                Method::PUT,
                &self.state.node,
                &path,
                Some(&body),
            )
            .await?;
        self.state.vault_at = now();
        self.save()
    }

    /// Toglie la cassaforte dal nodo e dimentica la password.
    pub async fn remove_vault(&mut self) -> Result<()> {
        let path = format!("/v1/inboxes/{}/vault", self.state.inbox);
        self.net
            .owner::<()>(
                &self.identity,
                Method::DELETE,
                &self.state.node,
                &path,
                None,
            )
            .await?;
        self.state.vault = None;
        self.state.vault_at = 0;
        self.save()
    }

    pub(crate) async fn vault_if_due(&mut self) -> Result<()> {
        if self.state.vault.is_none() || !self.has_node() {
            return Ok(());
        }
        if now() < self.state.vault_at.saturating_add(self.vault_every) {
            return Ok(());
        }
        self.backup_to_vault().await
    }

    /// Sul nodo c'è una cassaforte per questo nome o numero?
    pub async fn vault_exists(node: &str, who: &str) -> Result<bool> {
        let body = vault_who(who)?;
        match Net::new()?
            .call(Method::POST, node, "/v1/vault/find", &body)
            .await
        {
            Ok(_) => Ok(true),
            Err(Error::Node { status: 404, .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Ripristina il profilo dalla cassaforte del nodo (anche su un altro telefono): nome utente
    /// o numero verificato, e la password.
    pub async fn restore_from_vault(
        dir: &Path,
        node: &str,
        who: &str,
        password: &str,
    ) -> Result<Client> {
        if Client::exists(dir) {
            return Err(Error::Invalid(
                "in questa cartella c'è già un profilo".into(),
            ));
        }
        let net = Net::new()?;
        let mut body = {
            let who = who.to_string();
            tokio::task::spawn_blocking(move || vault_who(&who))
                .await
                .map_err(|e| Error::Invalid(format!("ripristino interrotto: {e}")))??
        };
        let found: VaultSalt = parse(
            &net.call(Method::POST, node, "/v1/vault/find", &body)
                .await?,
        )?;
        let keys = {
            let pw = password.to_string();
            tokio::task::spawn_blocking(move || vault::keys(&pw, &found.salt))
                .await
                .map_err(|e| Error::Invalid(format!("ripristino interrotto: {e}")))??
        };
        body["auth"] = json!(keys.auth);
        let opened: VaultOpened = parse(
            &net.call(Method::POST, node, "/v1/vault/open", &body)
                .await?,
        )?;
        let plain = vault::open(&keys, &b64::decode(&opened.blob)?)?;
        let mut client = Self::restore_payload(dir, &plain).await?;
        // Stessa password: la cassaforte continua da sola.
        client.state.vault = Some(keys);
        client.state.vault_at = now();
        client.save()?;
        Ok(client)
    }

    /// Ripristina un backup in una cartella vuota. La rete serve solo dopo: alla prima
    /// sincronizzazione il client riapre le chat e chiede di rientrare nei gruppi.
    pub async fn restore_backup(dir: &Path, file: &[u8], code: &str) -> Result<Client> {
        if Client::exists(dir) {
            return Err(Error::Invalid(
                "in questa cartella c'è già un profilo".into(),
            ));
        }
        let plain = {
            let (file, code) = (file.to_vec(), code.to_string());
            tokio::task::spawn_blocking(move || backup::open(&code, &file))
                .await
                .map_err(|e| Error::Invalid(format!("ripristino interrotto: {e}")))??
        };
        Self::restore_payload(dir, &plain).await
    }

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
