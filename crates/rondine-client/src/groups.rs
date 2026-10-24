//! Chat di gruppo (SPEC §7.4): chi scrive consegna lo stesso messaggio MLS a ogni membro.

use rondine_core::card::verify_card;
use rondine_core::envelope::{AppMessage, Envelope};
use rondine_core::{PublicKey, SignedCard, b64, check_version, now, random_id};

use crate::{Client, Error, Event, GroupChat, GroupMember, GroupMessage, Result};

/// Lunghezza massima del nome di un gruppo, in caratteri.
const MAX_GROUP_NAME: usize = 64;

/// Cosa è successo in un gruppo durante una sincronizzazione.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)] // eventi di breve vita: non vale la pena di un Box
pub enum GroupEvent {
    Joined {
        group: String,
        name: String,
        admin: PublicKey,
    },
    Message(GroupMessage),
    /// Sono entrati o usciti dei membri.
    MembersChanged {
        group: String,
    },
    /// Sono stato tolto dal gruppo.
    Removed {
        group: String,
    },
    /// Si è spostata (o ha smesso) una posizione in tempo reale nel gruppo (SPEC §7.6).
    LocationUpdate {
        group: String,
        id: String,
    },
    /// Un membro ha cambiato o tolto la foto (SPEC §7.7).
    PhotoChanged {
        identity: PublicKey,
    },
    /// Un membro ha cancellato per tutti questi suoi messaggi (SPEC §7.1b).
    Deleted {
        group: String,
        ids: Vec<String>,
    },
}

/// Esito di [`Client::sync_all`].
#[derive(Debug, Clone, Default)]
pub struct Synced {
    pub events: Vec<Event>,
    pub groups: Vec<GroupEvent>,
}

/// Esito dell'invio a un gruppo: a chi non è stato possibile consegnare.
#[derive(Debug, Clone)]
pub struct GroupDelivery {
    pub message: GroupMessage,
    pub undelivered: Vec<PublicKey>,
}

/// Azioni di rete da fare dopo aver elaborato la casella.
#[derive(Debug)]
pub(crate) enum Pending {
    /// Un membro ripristinato da un backup chiede di rientrare: lo tolgo e lo riaggiungo.
    Rejoin(String, PublicKey),
    /// Mandare al gruppo il mio biglietto con il permesso `gruppo`.
    Announce(String),
    /// Togliere un membro che ha chiesto di uscire (solo amministratore).
    Remove(String, PublicKey),
    /// Revocare il mio permesso di un gruppo da cui sono uscito.
    Revoke(String),
}

#[derive(Debug, Default)]
pub(crate) struct Batch {
    pub groups: Vec<GroupEvent>,
    pub pending: Vec<Pending>,
}

fn group_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_GROUP_NAME {
        return Err(Error::Invalid(format!(
            "il nome del gruppo deve avere da 1 a {MAX_GROUP_NAME} caratteri"
        )));
    }
    Ok(name.to_string())
}

fn gid_bytes(gid: &str) -> Result<Vec<u8>> {
    Ok(b64::decode(gid)?)
}

/// Mette nell'elenco il biglietto di un membro, se è più recente di quello che c'è.
/// `from_owner`: il biglietto l'ha mandato il membro stesso (`hello`), e a parità di data
/// vince su quello prestato dall'amministratore.
fn merge_card(
    g: &mut GroupChat,
    card: SignedCard,
    members: &[PublicKey],
    me: &PublicKey,
    from_owner: bool,
) {
    let Ok(info) = verify_card(&card) else {
        return;
    };
    if &info.identity == me || !members.contains(&info.identity) {
        return;
    }
    match g.members.iter_mut().find(|m| m.identity == info.identity) {
        Some(m) => {
            let newer = verify_card(&m.card).map_or(true, |old| {
                info.issued_at > old.issued_at || (from_owner && info.issued_at == old.issued_at)
            });
            if newer {
                m.display_name = info.display_name;
                m.card = card;
            }
        }
        None => g.members.push(GroupMember {
            identity: info.identity,
            display_name: info.display_name,
            card,
        }),
    }
}

/// Membri al massimo, me compreso (SPEC §7.4): ogni messaggio si deposita una volta per membro.
pub const MAX_GROUP_MEMBERS: usize = 100;

/// `others`: gli altri membri, io escluso.
fn check_size(others: usize) -> Result<()> {
    if others + 1 > MAX_GROUP_MEMBERS {
        return Err(Error::Invalid(format!(
            "un gruppo ha al massimo {MAX_GROUP_MEMBERS} membri"
        )));
    }
    Ok(())
}

impl Client {
    // ---------- lettura ----------

    pub fn groups(&self) -> &[GroupChat] {
        &self.state.group_chats
    }

    /// Trova un gruppo per nome (senza maiuscole) o per id.
    pub fn find_group(&self, query: &str) -> Result<&GroupChat> {
        let q = query.trim().to_lowercase();
        let matches: Vec<&GroupChat> = self
            .state
            .group_chats
            .iter()
            .filter(|g| g.id == query.trim() || g.name.to_lowercase() == q)
            .collect();
        match matches.as_slice() {
            [one] => Ok(one),
            [] => Err(Error::NotFound(format!("nessun gruppo «{query}»"))),
            _ => Err(Error::Invalid(format!(
                "«{query}» corrisponde a più gruppi: usa l'id"
            ))),
        }
    }

    pub fn group_messages(&self, gid: &str) -> Vec<&GroupMessage> {
        self.state
            .group_messages
            .iter()
            .filter(|m| m.group == gid)
            .collect()
    }

    pub fn mark_group_read(&mut self, gid: &str) -> Result<()> {
        let mut changed = false;
        for m in self
            .state
            .group_messages
            .iter_mut()
            .filter(|m| m.group == gid && !m.read)
        {
            m.read = true;
            changed = true;
        }
        if changed {
            self.save()?;
        }
        Ok(())
    }

    fn group_index(&self, gid: &str) -> Result<usize> {
        self.state
            .group_chats
            .iter()
            .position(|g| g.id == gid)
            .ok_or_else(|| Error::NotFound("gruppo sconosciuto".into()))
    }

    /// Indice di un gruppo attivo di cui sono amministratore.
    fn admin_group(&self, gid: &str) -> Result<usize> {
        let i = self.group_index(gid)?;
        let g = &self.state.group_chats[i];
        if !g.active {
            return Err(Error::Invalid("non fai più parte del gruppo".into()));
        }
        if g.admin != self.identity.public() {
            return Err(Error::Invalid(
                "solo l'amministratore può cambiare i membri".into(),
            ));
        }
        Ok(i)
    }

    /// Biglietti dei contatti da aggiungere a un gruppo: niente bloccati, doppioni o me.
    fn group_candidates(&self, ids: &[PublicKey]) -> Result<Vec<GroupMember>> {
        let me = self.identity.public();
        let mut out: Vec<GroupMember> = Vec::new();
        for id in ids {
            if id == &me || out.iter().any(|m| &m.identity == id) {
                continue;
            }
            let c = self
                .state
                .contacts
                .iter()
                .find(|c| &c.identity == id)
                .ok_or_else(|| Error::NotFound(format!("{} non è tra i contatti", id.short())))?;
            if c.blocked {
                return Err(Error::Invalid(format!("{} è bloccato", c.display_name)));
            }
            out.push(GroupMember {
                identity: c.identity,
                display_name: c.display_name.clone(),
                card: c.card.clone(),
            });
        }
        if out.is_empty() {
            return Err(Error::Invalid("serve almeno un altro membro".into()));
        }
        Ok(out)
    }

    async fn claim_all(&self, members: &[GroupMember]) -> Result<Vec<(Vec<u8>, PublicKey)>> {
        let mut kps = Vec::new();
        for m in members {
            kps.push((self.claim_keypackage(&m.card).await?, m.identity));
        }
        Ok(kps)
    }

    /// Il mio biglietto per un gruppo, con il permesso `gruppo` (lo chiede se manca). Senza
    /// nodo il biglietto non ha casella: mi raggiungono per le vie dirette e la mesh (SPEC §12).
    async fn my_group_card(&mut self, i: usize) -> Result<SignedCard> {
        if !self.has_node() {
            return self.card_with(None);
        }
        let permit = match self.state.group_chats[i].my_permit.clone() {
            Some(p) => p,
            None => {
                let p = self.new_permit("gruppo").await?;
                self.state.group_chats[i].my_permit = Some(p.clone());
                self.save()?;
                p
            }
        };
        self.card_with(Some(&permit))
    }

    /// Consegna la stessa busta a più membri, con le vie di SPEC §12.4 (nodo, diretto, coda e
    /// mesh); restituisce chi non l'ha ricevuta né l'ha in coda.
    async fn fanout(&mut self, to: &[GroupMember], envelope: &Envelope) -> Vec<PublicKey> {
        self.fanout_kind(to, envelope, crate::Kind::Normal).await
    }

    async fn fanout_kind(
        &mut self,
        to: &[GroupMember],
        envelope: &Envelope,
        kind: crate::Kind,
    ) -> Vec<PublicKey> {
        let mut failed = Vec::new();
        for m in to {
            if self
                .deposit_with(&m.card, envelope, kind, None)
                .await
                .is_err()
            {
                failed.push(m.identity);
            }
        }
        failed
    }

    /// Cifra un messaggio applicativo nel gruppo e salva subito l'archivio MLS.
    fn group_encrypt(&self, gid: &[u8], app: &AppMessage) -> Result<Envelope> {
        let ct = self.device.encrypt(gid, &serde_json::to_vec(app)?)?;
        self.save()?;
        Ok(Envelope::Mls {
            v: 1,
            msg: b64::encode(ct),
        })
    }

    // ---------- azioni ----------

    /// Crea un gruppo con alcuni contatti; io ne sono l'amministratore.
    pub async fn create_group(&mut self, name: &str, members: &[PublicKey]) -> Result<GroupChat> {
        let name = group_name(name)?;
        check_size(members.len())?;
        let members = self.group_candidates(members)?;
        let kps = self.claim_all(&members).await?;
        let permit = match self.has_node() {
            true => Some(self.new_permit("gruppo").await?),
            false => None,
        };
        let my_card = self.card_with(permit.as_ref())?;
        let me = self.identity.public();

        let (gid, welcome) = self.device.create_group(&kps)?;
        let mut cards = vec![my_card];
        cards.extend(members.iter().map(|m| m.card.clone()));
        let first = self.device.encrypt(
            &gid,
            &serde_json::to_vec(&AppMessage::Group {
                v: 1,
                name: name.clone(),
                admin: me,
                cards,
            })?,
        )?;
        let group = GroupChat {
            id: b64::encode(&gid),
            name,
            admin: me,
            members,
            active: true,
            my_permit: permit,
            created_at: now(),
            muted: false,
        };
        self.state.group_chats.push(group.clone());
        self.save()?;
        let envelope = Envelope::Welcome {
            v: 1,
            welcome: b64::encode(welcome),
            first: b64::encode(first),
        };
        let failed = self.fanout(&group.members, &envelope).await;
        if failed.len() == group.members.len() {
            return Err(Error::Invalid(
                "impossibile consegnare l'invito al gruppo a qualcuno".into(),
            ));
        }
        Ok(group)
    }

    /// Aggiunge contatti a un gruppo di cui sono amministratore.
    pub async fn add_group_members(&mut self, gid: &str, ids: &[PublicKey]) -> Result<()> {
        let i = self.admin_group(gid)?;
        let new: Vec<GroupMember> = self
            .group_candidates(ids)?
            .into_iter()
            .filter(|m| {
                !self.state.group_chats[i]
                    .members
                    .iter()
                    .any(|o| o.identity == m.identity)
            })
            .collect();
        if new.is_empty() {
            return Err(Error::Invalid("sono già tutti nel gruppo".into()));
        }
        check_size(self.state.group_chats[i].members.len() + new.len())?;
        let kps = self.claim_all(&new).await?;
        let my_card = self.my_group_card(i).await?;
        let raw = gid_bytes(gid)?;
        let (commit, welcome) = self.device.add_members(&raw, &kps)?;
        self.save()?;

        let old = self.state.group_chats[i].members.clone();
        let commit = Envelope::Mls {
            v: 1,
            msg: b64::encode(commit),
        };
        self.fanout(&old, &commit).await;

        // Elenco aggiornato: nel Welcome per i nuovi, come messaggio per gli altri.
        let g = &mut self.state.group_chats[i];
        g.members.extend(new.iter().cloned());
        let mut cards = vec![my_card];
        cards.extend(g.members.iter().map(|m| m.card.clone()));
        let roster = AppMessage::Group {
            v: 1,
            name: g.name.clone(),
            admin: g.admin,
            cards,
        };
        let first = self.device.encrypt(&raw, &serde_json::to_vec(&roster)?)?;
        self.save()?;
        let welcome = Envelope::Welcome {
            v: 1,
            welcome: b64::encode(welcome),
            first: b64::encode(&first),
        };
        self.fanout(&new, &welcome).await;
        self.fanout(
            &old,
            &Envelope::Mls {
                v: 1,
                msg: b64::encode(&first),
            },
        )
        .await;
        Ok(())
    }

    /// Toglie un membro da un gruppo di cui sono amministratore.
    pub async fn remove_group_member(&mut self, gid: &str, who: &PublicKey) -> Result<()> {
        let i = self.admin_group(gid)?;
        if !self.state.group_chats[i]
            .members
            .iter()
            .any(|m| &m.identity == who)
        {
            return Err(Error::NotFound("non è nel gruppo".into()));
        }
        let commit = self.device.remove_members(&gid_bytes(gid)?, &[*who])?;
        self.save()?;
        // Il commit va anche a chi esce, così sa di essere stato tolto.
        let all = self.state.group_chats[i].members.clone();
        self.fanout(
            &all,
            &Envelope::Mls {
                v: 1,
                msg: b64::encode(commit),
            },
        )
        .await;
        self.state.group_chats[i]
            .members
            .retain(|m| &m.identity != who);
        self.save()
    }

    /// Passa il ruolo di amministratore a un altro membro (SPEC §7.4).
    pub async fn set_group_admin(&mut self, gid: &str, who: &PublicKey) -> Result<()> {
        let i = self.group_index(gid)?;
        let g = self.state.group_chats[i].clone();
        if !g.active {
            return Err(Error::Invalid("non fai più parte del gruppo".into()));
        }
        if g.admin != self.identity.public() {
            return Err(Error::Invalid(
                "solo l'amministratore passa il ruolo".into(),
            ));
        }
        if !g.members.iter().any(|m| &m.identity == who) {
            return Err(Error::NotFound("non è nel gruppo".into()));
        }
        let my_card = self.my_group_card(i).await?;
        let mut cards = vec![my_card];
        cards.extend(g.members.iter().map(|m| m.card.clone()));
        let roster = AppMessage::Group {
            v: 1,
            name: g.name.clone(),
            admin: *who,
            cards,
        };
        let envelope = self.group_encrypt(&gid_bytes(gid)?, &roster)?;
        self.state.group_chats[i].admin = *who;
        self.save()?;
        self.fanout(&g.members, &envelope).await;
        Ok(())
    }

    /// Esce da un gruppo: chiede all'amministratore di togliermi e smette subito di ricevere
    /// (il permesso `gruppo` si revoca). L'amministratore prima passa il ruolo al primo degli
    /// altri membri; se è rimasto solo, il gruppo finisce.
    pub async fn leave_group(&mut self, gid: &str) -> Result<()> {
        let i = self.group_index(gid)?;
        let mut g = self.state.group_chats[i].clone();
        if !g.active {
            return Err(Error::Invalid("non fai più parte del gruppo".into()));
        }
        if g.admin == self.identity.public()
            && let Some(next) = g.members.first().map(|m| m.identity)
        {
            self.set_group_admin(gid, &next).await?;
            g = self.state.group_chats[i].clone();
        }
        let admin: Vec<GroupMember> = g
            .members
            .iter()
            .filter(|m| m.identity == g.admin)
            .cloned()
            .collect();
        if !admin.is_empty() {
            // Se l'amministratore non si raggiunge si esce lo stesso: col permesso revocato
            // nessuno può più consegnarmi i messaggi del gruppo.
            let envelope = self.group_encrypt(&gid_bytes(gid)?, &AppMessage::Leave { v: 1 })?;
            self.fanout(&admin, &envelope).await;
        }
        let permit = {
            let g = &mut self.state.group_chats[i];
            g.active = false;
            g.my_permit.take()
        };
        self.save()?;
        if let Some(p) = permit {
            self.revoke_permit(&p.verify()?.id).await?;
        }
        Ok(())
    }

    pub async fn send_group_text(&mut self, gid: &str, body: &str) -> Result<GroupDelivery> {
        self.send_group_reply(gid, body, None).await
    }

    /// Un testo nel gruppo che risponde a un altro messaggio del gruppo (SPEC §7.1b).
    pub async fn send_group_reply(
        &mut self,
        gid: &str,
        body: &str,
        reply_to: Option<&str>,
    ) -> Result<GroupDelivery> {
        let body = body.trim();
        if body.is_empty() {
            return Err(Error::Invalid("messaggio vuoto".into()));
        }
        let g = self.state.group_chats[self.group_index(gid)?].clone();
        if !g.active {
            return Err(Error::Invalid("non fai più parte del gruppo".into()));
        }
        let message = GroupMessage {
            id: random_id(),
            group: g.id.clone(),
            sender: self.identity.public(),
            outgoing: true,
            body: body.to_string(),
            ts: now(),
            read: true,
            attachment: None,
            location: None,
            reply_to: reply_to.map(str::to_string),
        };
        let envelope = self.group_encrypt(
            &gid_bytes(gid)?,
            &AppMessage::Text {
                v: 1,
                id: message.id.clone(),
                ts: message.ts,
                body: message.body.clone(),
                reply: message.reply_to.clone(),
            },
        )?;
        let undelivered = self.fanout(&g.members, &envelope).await;
        if !g.members.is_empty() && undelivered.len() == g.members.len() {
            return Err(Error::Invalid(
                "impossibile consegnare il messaggio a qualcuno".into(),
            ));
        }
        self.state.group_messages.push(message.clone());
        self.save()?;
        Ok(GroupDelivery {
            message,
            undelivered,
        })
    }

    /// Cancella messaggi di un gruppo: per me sempre; con `for_everyone` chiede agli altri di
    /// cancellare quelli che ho mandato io.
    pub async fn delete_group_messages(
        &mut self,
        gid: &str,
        ids: &[String],
        for_everyone: bool,
    ) -> Result<()> {
        let mine: Vec<String> = self
            .state
            .group_messages
            .iter()
            .filter(|m| m.group == gid && m.outgoing && ids.contains(&m.id))
            .map(|m| m.id.clone())
            .collect();
        self.state
            .group_messages
            .retain(|m| !(m.group == gid && ids.contains(&m.id)));
        for id in ids {
            self.remove_files(id);
        }
        self.save()?;
        if for_everyone && !mine.is_empty() {
            for part in mine.chunks(100) {
                self.group_send(
                    gid,
                    &AppMessage::Delete {
                        v: 1,
                        ids: part.to_vec(),
                    },
                    crate::Kind::Normal,
                )
                .await?;
            }
        }
        Ok(())
    }

    /// Svuota la chat di un gruppo, solo su questo telefono.
    pub fn clear_group_chat(&mut self, gid: &str) -> Result<()> {
        let ids: Vec<String> = self
            .state
            .group_messages
            .iter()
            .filter(|m| m.group == gid)
            .map(|m| m.id.clone())
            .collect();
        self.state.group_messages.retain(|m| m.group != gid);
        for id in &ids {
            self.remove_files(id);
        }
        self.save()
    }

    /// Toglie dalla lista un gruppo, con i messaggi: uno da cui sono uscito (o sono stato
    /// tolto), oppure uno in cui sono rimasto solo, che così finisce.
    pub async fn delete_group(&mut self, gid: &str) -> Result<()> {
        let i = self.group_index(gid)?;
        let g = &self.state.group_chats[i];
        if g.active {
            if !g.members.is_empty() {
                return Err(Error::Invalid("prima esci dal gruppo".into()));
            }
            self.leave_group(gid).await?;
        }
        self.clear_group_chat(gid)?;
        self.state.group_chats.retain(|g| g.id != gid);
        self.save()
    }

    /// Richiesta di rientro dopo un ripristino (SPEC §7.8): vale se sono l'amministratore e chi
    /// chiede è tra i membri del gruppo.
    pub(crate) fn received_rejoin(&mut self, sender: PublicKey, gid: &str) {
        let me = self.identity.public();
        let ok = self.state.group_chats.iter().any(|g| {
            g.id == gid
                && g.active
                && g.admin == me
                && g.members.iter().any(|m| m.identity == sender)
        });
        if ok {
            self.batch
                .pending
                .push(Pending::Rejoin(gid.to_string(), sender));
        }
    }

    /// Cifra un messaggio nel gruppo e lo consegna a tutti i membri.
    pub(crate) async fn group_send(
        &mut self,
        gid: &str,
        app: &AppMessage,
        kind: crate::Kind,
    ) -> Result<()> {
        let g = self.state.group_chats[self.group_index(gid)?].clone();
        if !g.active {
            return Err(Error::Invalid("non fai più parte del gruppo".into()));
        }
        let envelope = self.group_encrypt(&gid_bytes(gid)?, app)?;
        let failed = self.fanout_kind(&g.members, &envelope, kind).await;
        if !g.members.is_empty() && failed.len() == g.members.len() {
            return Err(Error::Invalid(
                "impossibile consegnare il messaggio a qualcuno".into(),
            ));
        }
        Ok(())
    }

    /// Foto, documento o vocale nel gruppo (SPEC §7.5): se è grande, una sola copia cifrata tra
    /// i file condivisi del mio nodo, che ogni membro scarica da lì.
    pub async fn send_group_file(
        &mut self,
        gid: &str,
        file: crate::NewFile,
    ) -> Result<GroupMessage> {
        let i = self.group_index(gid)?;
        if !self.state.group_chats[i].active {
            return Err(Error::Invalid("non fai più parte del gruppo".into()));
        }
        let (f, attachment) = self.seal_file(&file, crate::FileDest::Shared).await?;
        let message = GroupMessage {
            id: f.id.clone(),
            group: gid.to_string(),
            sender: self.identity.public(),
            outgoing: true,
            body: f.caption.clone().unwrap_or_default(),
            ts: f.ts,
            read: true,
            attachment: Some(attachment),
            location: None,
            reply_to: None,
        };
        self.group_send(gid, &AppMessage::File(Box::new(f)), crate::Kind::Normal)
            .await?;
        self.state.group_messages.push(message.clone());
        self.save()?;
        Ok(message)
    }

    /// Posizione nel gruppo, fissa o in tempo reale (SPEC §7.6); si aggiorna con
    /// [`Client::update_location`] come nelle chat 1:1.
    pub async fn send_group_location(
        &mut self,
        gid: &str,
        lat: f64,
        lon: f64,
        acc: Option<f32>,
        live_for: Option<u64>,
    ) -> Result<GroupMessage> {
        let ts = now();
        let loc = rondine_core::envelope::LocationMsg {
            v: 1,
            id: random_id(),
            ts,
            lat,
            lon,
            acc,
            live_until: live_for.map(|s| ts.saturating_add(s)),
        };
        if !loc.valid() || live_for == Some(0) {
            return Err(Error::Invalid("posizione o durata non valida".into()));
        }
        let message = GroupMessage {
            id: loc.id.clone(),
            group: gid.to_string(),
            sender: self.identity.public(),
            outgoing: true,
            body: String::new(),
            ts,
            read: true,
            attachment: None,
            location: Some(crate::Location {
                lat,
                lon,
                acc,
                live_until: loc.live_until,
                updated_at: ts,
            }),
            reply_to: None,
        };
        self.group_send(gid, &AppMessage::Location(loc), crate::Kind::Normal)
            .await?;
        self.state.group_messages.push(message.clone());
        self.save()?;
        Ok(message)
    }

    // ---------- ricezione ----------

    /// Welcome di un gruppo: il primo messaggio è l'elenco mandato dall'amministratore.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn join_group(
        &mut self,
        members: &[PublicKey],
        gid: &[u8],
        from: PublicKey,
        name: String,
        admin: PublicKey,
        cards: Vec<SignedCard>,
    ) -> Result<()> {
        let me = self.identity.public();
        let name = group_name(&name)?;
        if from != admin || !members.contains(&admin) || !members.contains(&me) {
            return Err(Error::Invalid("invito al gruppo non valido".into()));
        }
        if self
            .state
            .contacts
            .iter()
            .any(|c| c.identity == admin && c.blocked)
        {
            return Err(Error::Invalid("invito da un contatto bloccato".into()));
        }
        let id = b64::encode(gid);
        // Riaggiunto dopo essere stato tolto: il gruppo vecchio (non attivo) lascia il posto al
        // nuovo, i messaggi restano. Uno attivo non si sostituisce.
        let mut created_at = now();
        if let Some(i) = self.state.group_chats.iter().position(|g| g.id == id) {
            if self.state.group_chats[i].active {
                return Err(Error::Invalid("gruppo già presente".into()));
            }
            created_at = self.state.group_chats.remove(i).created_at;
        }
        let mut g = GroupChat {
            id: id.clone(),
            name: name.clone(),
            admin,
            members: Vec::new(),
            active: true,
            my_permit: None,
            created_at,
            muted: false,
        };
        for card in cards {
            merge_card(&mut g, card, members, &me, false);
        }
        if !g.members.iter().any(|m| m.identity == admin) {
            return Err(Error::Invalid(
                "manca il biglietto dell'amministratore".into(),
            ));
        }
        self.state.group_chats.push(g);
        self.batch.groups.push(GroupEvent::Joined {
            group: id.clone(),
            name,
            admin,
        });
        self.batch.pending.push(Pending::Announce(id));
        Ok(())
    }

    /// Un commit dell'amministratore ha cambiato i membri.
    pub(crate) fn group_changed(&mut self, gid: &[u8]) -> Result<()> {
        let id = b64::encode(gid);
        let Ok(i) = self.group_index(&id) else {
            return Ok(());
        };
        let members = self.device.members(gid)?;
        let still_member = self.device.is_member(gid)?;
        let g = &mut self.state.group_chats[i];
        let before = g.members.len();
        g.members.retain(|m| members.contains(&m.identity));
        if !still_member {
            if g.active {
                g.active = false;
                self.batch.groups.push(GroupEvent::Removed { group: id });
                if let Some(p) = g.my_permit.take() {
                    self.batch.pending.push(Pending::Revoke(p.verify()?.id));
                }
            }
            return Ok(());
        }
        if g.active {
            // Ai nuovi membri arriva il mio biglietto `gruppo`.
            if members.len() > before + 1 {
                self.batch.pending.push(Pending::Announce(id.clone()));
            }
            self.batch
                .groups
                .push(GroupEvent::MembersChanged { group: id });
        }
        Ok(())
    }

    pub(crate) fn group_app_message(
        &mut self,
        gid: &str,
        sender: PublicKey,
        plaintext: &[u8],
    ) -> Result<()> {
        let i = self.group_index(gid)?;
        if !self.state.group_chats[i].active {
            return Ok(());
        }
        if self
            .state
            .contacts
            .iter()
            .any(|c| c.identity == sender && c.blocked)
        {
            return Ok(());
        }
        let app: AppMessage = serde_json::from_slice(plaintext)?;
        check_version(app.version())?;
        let me = self.identity.public();
        match app {
            AppMessage::Text {
                id,
                ts,
                body,
                reply,
                ..
            } => {
                if self.state.group_messages.iter().any(|m| m.id == id) {
                    return Ok(());
                }
                let m = GroupMessage {
                    id,
                    group: gid.to_string(),
                    sender,
                    outgoing: false,
                    body,
                    ts,
                    read: false,
                    attachment: None,
                    location: None,
                    reply_to: reply,
                };
                self.state.group_messages.push(m.clone());
                self.batch.groups.push(GroupEvent::Message(m));
            }
            AppMessage::File(f) => {
                if self.state.group_messages.iter().any(|m| m.id == f.id) {
                    return Ok(());
                }
                let attachment = self.received_attachment(&f)?;
                let m = GroupMessage {
                    id: f.id,
                    group: gid.to_string(),
                    sender,
                    outgoing: false,
                    body: f.caption.unwrap_or_default(),
                    ts: f.ts,
                    read: false,
                    attachment: Some(attachment),
                    location: None,
                    reply_to: None,
                };
                self.state.group_messages.push(m.clone());
                self.batch.groups.push(GroupEvent::Message(m));
            }
            AppMessage::Location(l) => {
                if !l.valid() {
                    return Err(Error::Invalid("posizione non valida".into()));
                }
                if self.state.group_messages.iter().any(|m| m.id == l.id) {
                    return Ok(());
                }
                let m = GroupMessage {
                    id: l.id,
                    group: gid.to_string(),
                    sender,
                    outgoing: false,
                    body: String::new(),
                    ts: l.ts,
                    read: false,
                    attachment: None,
                    location: Some(crate::Location {
                        lat: l.lat,
                        lon: l.lon,
                        acc: l.acc,
                        live_until: l.live_until,
                        updated_at: l.ts,
                    }),
                    reply_to: None,
                };
                self.state.group_messages.push(m.clone());
                self.batch.groups.push(GroupEvent::Message(m));
            }
            AppMessage::Delete { ids, .. } => {
                // Solo i messaggi di chi chiede (SPEC §7.1b).
                let ids: Vec<String> = ids.into_iter().take(100).collect();
                let gone: Vec<String> = self
                    .state
                    .group_messages
                    .iter()
                    .filter(|m| {
                        m.group == gid && m.sender == sender && !m.outgoing && ids.contains(&m.id)
                    })
                    .map(|m| m.id.clone())
                    .collect();
                if !gone.is_empty() {
                    self.state
                        .group_messages
                        .retain(|m| !(m.group == gid && gone.contains(&m.id)));
                    for id in &gone {
                        self.remove_files(id);
                    }
                    self.batch.groups.push(GroupEvent::Deleted {
                        group: gid.to_string(),
                        ids: gone,
                    });
                }
            }
            AppMessage::Profile {
                ts,
                photo,
                name,
                about,
                ..
            } => {
                // Anche da chi non è un contatto: lo si vede nel gruppo (SPEC §7.7).
                let ev = self.received_profile(sender, ts, photo, name, about)?;
                if !ev.is_empty() {
                    self.batch
                        .groups
                        .push(GroupEvent::PhotoChanged { identity: sender });
                }
            }
            AppMessage::LocationUpdate(l) => {
                // Stesse regole della chat 1:1 (SPEC §7.6): stesso mittente, non più vecchia,
                // condivisione non finita, la fine può solo anticipare.
                if !l.valid() {
                    return Err(Error::Invalid("posizione non valida".into()));
                }
                let Some(loc) = self
                    .state
                    .group_messages
                    .iter_mut()
                    .find(|m| m.id == l.id && m.group == gid && m.sender == sender && !m.outgoing)
                    .and_then(|m| m.location.as_mut())
                else {
                    return Ok(());
                };
                let Some(until) = loc.live_until else {
                    return Ok(());
                };
                if l.ts < loc.updated_at || l.ts > until {
                    return Ok(());
                }
                *loc = crate::Location {
                    lat: l.lat,
                    lon: l.lon,
                    acc: l.acc,
                    live_until: Some(l.live_until.map_or(until, |u| u.min(until))),
                    updated_at: l.ts,
                };
                self.batch.groups.push(GroupEvent::LocationUpdate {
                    group: gid.to_string(),
                    id: l.id,
                });
            }
            AppMessage::Hello { card, .. } => {
                if verify_card(&card)?.identity != sender {
                    return Err(Error::Invalid("biglietto di un altro".into()));
                }
                let members = self.device.members(&gid_bytes(gid)?)?;
                merge_card(&mut self.state.group_chats[i], card, &members, &me, true);
            }
            AppMessage::Group {
                name, admin, cards, ..
            } => {
                let g = &self.state.group_chats[i];
                if sender != g.admin {
                    return Err(Error::Invalid(
                        "elenco del gruppo non dall'amministratore".into(),
                    ));
                }
                let name = group_name(&name)?;
                let members = self.device.members(&gid_bytes(gid)?)?;
                // Passaggio del ruolo (SPEC §7.4): solo a un membro del gruppo.
                if admin != g.admin {
                    if !members.contains(&admin) {
                        return Err(Error::Invalid("nuovo amministratore non nel gruppo".into()));
                    }
                    self.batch.groups.push(GroupEvent::MembersChanged {
                        group: gid.to_string(),
                    });
                }
                let g = &mut self.state.group_chats[i];
                g.admin = admin;
                g.name = name;
                for card in cards {
                    merge_card(g, card, &members, &me, false);
                }
            }
            AppMessage::Leave { .. } if self.state.group_chats[i].admin == me => {
                self.batch
                    .pending
                    .push(Pending::Remove(gid.to_string(), sender));
            }
            // Spunte, "sta scrivendo", indirizzi e mesh nei gruppi non ci sono ancora.
            _ => {}
        }
        Ok(())
    }

    /// Fa le azioni di rete raccolte durante la sincronizzazione.
    pub(crate) async fn run_pending(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        for p in std::mem::take(&mut self.batch.pending) {
            let res = match &p {
                Pending::Announce(gid) => self.announce(gid).await,
                Pending::Remove(gid, who) => self.remove_group_member(gid, who).await,
                Pending::Revoke(id) => self.revoke_permit(id).await,
                Pending::Rejoin(gid, who) => {
                    let _ = self.remove_group_member(gid, who).await;
                    self.add_group_members(gid, std::slice::from_ref(who)).await
                }
            };
            if let Err(e) = res {
                events.push(Event::Rejected(format!("gruppo: {e}")));
            }
        }
        events
    }

    /// Manda al gruppo il mio biglietto con il permesso `gruppo`.
    async fn announce(&mut self, gid: &str) -> Result<()> {
        let i = self.group_index(gid)?;
        if !self.state.group_chats[i].active {
            return Ok(());
        }
        let card = self.my_group_card(i).await?;
        let envelope = self.group_encrypt(
            &gid_bytes(gid)?,
            &AppMessage::Hello {
                v: 1,
                card,
                reset: false,
            },
        )?;
        let members = self.state.group_chats[i].members.clone();
        self.fanout(&members, &envelope).await;
        // Con il biglietto anche la foto, se ce l'ho (SPEC §7.7).
        if let Some(app) = self.profile_message() {
            let envelope = self.group_encrypt(&gid_bytes(gid)?, &app)?;
            self.fanout(&members, &envelope).await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rondine_core::permit::{Permit, Scope};
    use rondine_core::{Card, Identity, Signed};

    use super::*;

    fn card(who: &Identity, label: &str, issued_at: u64) -> Result<SignedCard> {
        let node = Identity::generate();
        let permit = Signed::sign(
            &node,
            &Permit {
                v: 1,
                id: random_id(),
                node_key: node.public(),
                inbox: "casella".into(),
                label: label.into(),
                scopes: Scope::ALL.to_vec(),
                issued_at,
                expires_at: None,
            },
        )?;
        Ok(Signed::sign(
            who,
            &Card {
                v: 1,
                identity: who.public(),
                display_name: "Carla".into(),
                username: None,
                node: Some("http://nodo".into()),
                inbox: Some("casella".into()),
                permit: Some(permit),
                issued_at,
            },
        )?)
    }

    fn label(g: &GroupChat) -> Result<String> {
        let card = verify_card(&g.members[0].card)?;
        let permit = card
            .permit
            .ok_or(Error::Invalid("biglietto senza permesso".into()))?;
        Ok(permit.verify()?.label)
    }

    /// Il biglietto `gruppo` mandato dal membro vince su quello prestato dall'amministratore
    /// anche se è dello stesso secondo (bug: a parità restava quello vecchio).
    #[test]
    fn biglietto_del_membro_vince_a_parita_di_data() -> Result<()> {
        let (me, carla) = (Identity::generate().public(), Identity::generate());
        let mut g = GroupChat {
            id: "g".into(),
            name: "G".into(),
            admin: me,
            members: Vec::new(),
            active: true,
            my_permit: None,
            created_at: 0,
            muted: false,
        };
        let members = [me, carla.public()];
        merge_card(
            &mut g,
            card(&carla, "contatto:anna", 100)?,
            &members,
            &me,
            false,
        );
        merge_card(&mut g, card(&carla, "gruppo", 100)?, &members, &me, true);
        assert_eq!(label(&g)?, "gruppo");
        // Un elenco dell'amministratore con un biglietto non più recente non lo sostituisce.
        merge_card(
            &mut g,
            card(&carla, "contatto:anna", 100)?,
            &members,
            &me,
            false,
        );
        assert_eq!(label(&g)?, "gruppo");
        // Uno più vecchio non vince mai.
        merge_card(&mut g, card(&carla, "vecchio", 99)?, &members, &me, true);
        assert_eq!(label(&g)?, "gruppo");
        Ok(())
    }
}
