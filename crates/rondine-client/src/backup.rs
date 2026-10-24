//! Backup e ripristino del profilo (SPEC §7.8).
//!
//! Il backup contiene identità, contatti, messaggi, gruppi e foto del profilo, cifrati con un
//! codice di 30 cifre. Non contiene lo stato MLS: dopo il ripristino le chat 1:1 si riaprono con
//! un `hello` di reset e si chiede agli amministratori di riaggiungerci ai gruppi.

use std::collections::BTreeMap;
use std::path::Path;

use reqwest::Method;
use rondine_core::envelope::AppMessage;
use rondine_core::{Identity, b64, backup, now};
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

impl Client {
    /// Il backup cifrato e il codice per aprirlo (da mostrare una volta e far annotare).
    pub fn export_backup(&self) -> Result<(Vec<u8>, String)> {
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
        let code = backup::new_code();
        let file = backup::seal(&code, &serde_json::to_vec(&payload)?)?;
        Ok((file, code))
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
        let payload: Payload = serde_json::from_slice(&plain)?;
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
