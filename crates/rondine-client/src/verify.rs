//! Verifica del numero tra amici (SPEC §14.1): chi verifica fa uno squillo (o manda un SMS) al
//! numero che ha in rubrica, e la prova torna dalla chiave da verificare nella chat cifrata. Una
//! chiave falsa data da qualcuno nel mezzo non riceve né squillo né SMS: suonano sulla SIM vera.
//! Il nodo non vede né numeri né codici.
//!
//! Le chiamate e gli SMS li fa il telefono (l'app Android); qui ci sono il piano, il confronto
//! e i messaggi nella chat.

use rondine_core::envelope::{AppMessage, RingSeen, VerifyMethod, caller_print};
use rondine_core::{PublicKey, b64, now, phone, random_id};
use serde::{Deserialize, Serialize};

use crate::{Client, Error, Event, Kind, Result};

/// Ritardo accettato tra la partenza della chiamata e lo squillo dall'altra parte (la rete
/// mobile impiega qualche secondo), più lo scarto tra i due orologi di rete.
const LAG_SECS: std::ops::RangeInclusive<i64> = -2..=10;
/// Impronte accettate nella risposta (fino a 5 chiamate, ognuna in due letture del numero).
const MAX_CALLS: usize = 10;
/// Dopo quanto una verifica o una richiesta non vale più.
const VERIFY_TTL_SECS: u64 = 15 * 60;
/// Dopo quanti secondi dalla richiesta parte lo squillo: il tempo che la sveglia arrivi al
/// telefono da verificare e che il suo filtro chiamate si prepari.
const RING_AFTER_SECS: u64 = 6;
/// Dove sta il codice nell'SMS: il link che l'app riconosce.
const SMS_LINK: &str = "rondine:verifica/";

/// Una verifica che ho avviato io (SPEC §14.1).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Verification {
    pub id: String,
    pub contact: PublicKey,
    /// Il numero che chiamo o a cui scrivo, in E.164.
    pub number: String,
    pub method: VerifyMethod,
    pub started_at: u64,
    /// Squillo: il mio numero (E.164), da cui parte la chiamata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub my_number: Option<String>,
    /// Squillo: quando è partita la mia chiamata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rang_at: Option<u64>,
    /// SMS: il codice mandato.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// Un contatto sta per verificare il mio numero.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct VerifyRequest {
    pub id: String,
    pub from: PublicKey,
    pub method: VerifyMethod,
    pub received_at: u64,
}

/// Cosa deve fare il telefono di chi verifica.
#[derive(Clone, Debug)]
pub struct VerifyPlan {
    pub id: String,
    /// Il numero da chiamare o a cui scrivere, in E.164.
    pub number: String,
    pub method: VerifyMethod,
    /// Squillo: dopo quanti secondi da adesso chiamare (poi `record_ring`).
    pub delays: Vec<u64>,
    /// SMS: il codice e il testo da mandare dalla mia SIM.
    pub code: Option<String>,
    pub sms_text: Option<String>,
}

fn random_u64() -> Result<u64> {
    let bytes = b64::decode(&random_id())?;
    let first: [u8; 8] = bytes
        .get(..8)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| Error::Invalid("numero casuale non disponibile".into()))?;
    Ok(u64::from_le_bytes(first))
}

/// Lo squillo torna? Una chiamata dal mio numero (impronta legata alla verifica) arrivata con il
/// ritardo della rete rispetto a quando l'ho fatta partire (SPEC §14.1).
fn ring_matches(id: &str, my_number: &str, rang_at: u64, calls: &[RingSeen]) -> bool {
    if calls.len() > MAX_CALLS {
        return false;
    }
    let print = caller_print(id, my_number);
    let Ok(rang) = i64::try_from(rang_at) else {
        return false;
    };
    calls.iter().any(|c| {
        i64::try_from(c.at).is_ok_and(|at| LAG_SECS.contains(&(at - rang))) && c.caller == print
    })
}

/// Le letture possibili di un numero come lo scrive il registro chiamate: col prefisso del
/// paese predefinito e, se manca il `+`, anche come numero internazionale (`15555215556` può
/// essere `+15555215556`).
fn readings(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Ok(n) = phone::normalize(raw, phone::DEFAULT_COUNTRY) {
        out.push(n);
    }
    let t = raw.trim();
    if !t.starts_with('+')
        && !t.starts_with("00")
        && let Ok(n) = phone::normalize(&format!("+{t}"), phone::DEFAULT_COUNTRY)
        && !out.contains(&n)
    {
        out.push(n);
    }
    out
}

/// Il codice in un SMS di verifica: le 8 cifre dopo il link.
fn code_in(text: &str) -> Option<String> {
    let rest = &text[text.find(SMS_LINK)? + SMS_LINK.len()..];
    let code: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    (code.len() == 8).then_some(code)
}

impl Client {
    /// Avvia la verifica del numero di un contatto. `number`: quello in rubrica, se il contatto
    /// non ce l'ha già. Avvisa il contatto nella chat e restituisce il piano per il telefono.
    /// Lo squillo vuole il mio numero nel profilo: la prova è che la chiamata arrivi da lì.
    pub async fn start_verify(
        &mut self,
        contact: &PublicKey,
        method: VerifyMethod,
        number: Option<&str>,
    ) -> Result<VerifyPlan> {
        let c = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == contact && !c.blocked)
            .cloned()
            .ok_or_else(|| Error::NotFound("contatto sconosciuto".into()))?;
        let raw = number
            .map(str::to_string)
            .or_else(|| c.phone.clone())
            .ok_or_else(|| Error::Invalid("serve il numero del contatto".into()))?;
        let number = phone::normalize(&raw, phone::DEFAULT_COUNTRY)?;
        let id = random_id();
        let (delays, my_number, code, sms_text) = match method {
            VerifyMethod::Ring => {
                let mine = self
                    .state
                    .phone
                    .as_deref()
                    .map(|p| phone::normalize(p, phone::DEFAULT_COUNTRY))
                    .transpose()?
                    .ok_or_else(|| {
                        Error::Invalid("metti prima il tuo numero: lo squillo parte da lì".into())
                    })?;
                (vec![RING_AFTER_SECS], Some(mine), None, None)
            }
            VerifyMethod::Sms => {
                let code = format!("{:08}", random_u64()? % 100_000_000);
                let text = format!(
                    "Rondine: {} ti chiede di confermare il tuo numero. Codice {code} {SMS_LINK}{code}",
                    self.state.display_name
                );
                (Vec::new(), None, Some(code), Some(text))
            }
        };
        let t = now();
        // Una verifica alla volta per contatto; le scadute si buttano.
        self.state
            .verifications
            .retain(|v| v.started_at + VERIFY_TTL_SECS > t && v.contact != *contact);
        self.state.verifications.push(Verification {
            id: id.clone(),
            contact: *contact,
            number: number.clone(),
            method,
            started_at: t,
            my_number,
            rang_at: None,
            code: code.clone(),
        });
        self.save()?;
        let app = AppMessage::VerifyRequest {
            v: 1,
            id: id.clone(),
            method,
        };
        // Sveglia il telefono da verificare: deve aspettarsi lo squillo (o l'SMS).
        self.send_app_with(&c, &app, Kind::Normal, None).await?;
        Ok(VerifyPlan {
            id,
            number,
            method,
            delays,
            code,
            sms_text,
        })
    }

    /// Il telefono ha fatto partire lo squillo della verifica `id` all'istante `at`.
    pub fn record_ring(&mut self, id: &str, at: u64) -> Result<()> {
        let v = self
            .state
            .verifications
            .iter_mut()
            .find(|v| v.id == id && v.method == VerifyMethod::Ring)
            .ok_or_else(|| Error::NotFound("verifica sconosciuta".into()))?;
        v.rang_at = Some(at);
        self.save()
    }

    /// Le richieste di verifica del mio numero ancora valide.
    pub fn verify_requests(&self) -> Vec<VerifyRequest> {
        let t = now();
        self.state
            .verify_requests
            .iter()
            .filter(|r| r.received_at + VERIFY_TTL_SECS > t)
            .cloned()
            .collect()
    }

    /// Risponde agli squilli in attesa con le chiamate arrivate: istante e numero come li ha
    /// visti il telefono (il numero non parte: solo la sua impronta per ogni richiesta). Ogni
    /// chi verifica la confronta col suo numero. Restituisce a quante richieste ha risposto.
    pub async fn answer_rings(&mut self, calls: &[(u64, String)]) -> Result<usize> {
        let pending: Vec<VerifyRequest> = self
            .verify_requests()
            .into_iter()
            .filter(|r| r.method == VerifyMethod::Ring)
            .collect();
        let mut answered: Vec<String> = Vec::new();
        for r in &pending {
            let seen: Vec<RingSeen> = calls
                .iter()
                .flat_map(|(at, raw)| {
                    readings(raw).into_iter().map(|n| RingSeen {
                        at: *at,
                        caller: caller_print(&r.id, &n),
                    })
                })
                .take(MAX_CALLS)
                .collect();
            if seen.is_empty() {
                continue;
            }
            if self.reply_to(&r.from, &r.id, seen, None, false).await {
                answered.push(r.id.clone());
            }
        }
        self.state
            .verify_requests
            .retain(|r| !answered.contains(&r.id));
        self.save()?;
        Ok(answered.len())
    }

    /// Questo telefono non sa ricevere uno squillo di verifica (niente filtro chiamate né
    /// registro): chi verifica passa all'SMS.
    pub async fn answer_unable(&mut self, id: &str) -> Result<()> {
        let req = self
            .state
            .verify_requests
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or_else(|| Error::NotFound("richiesta di verifica sconosciuta".into()))?;
        self.reply_to(&req.from, id, Vec::new(), None, true).await;
        self.state.verify_requests.retain(|r| r.id != id);
        self.save()
    }

    /// Il codice di un SMS di verifica arrivato sul mio telefono: va a chi ha chiesto una
    /// verifica via SMS (a chi l'ha mandato torna giusto, agli altri non serve). Restituisce a
    /// quanti contatti è andato.
    pub async fn answer_sms_code(&mut self, code: &str) -> Result<usize> {
        let code = code.trim().to_string();
        let pending: Vec<VerifyRequest> = self
            .verify_requests()
            .into_iter()
            .filter(|r| r.method == VerifyMethod::Sms)
            .collect();
        let mut sent = 0;
        for r in &pending {
            if self
                .reply_to(&r.from, &r.id, Vec::new(), Some(code.clone()), false)
                .await
            {
                sent += 1;
            }
        }
        self.state
            .verify_requests
            .retain(|r| !pending.iter().any(|p| p.id == r.id));
        self.save()?;
        Ok(sent)
    }

    /// Come [`Client::answer_sms_code`], dal testo intero dell'SMS.
    pub async fn answer_sms_text(&mut self, text: &str) -> Result<usize> {
        let code = code_in(text)
            .ok_or_else(|| Error::Invalid("non è un SMS di verifica di Rondine".into()))?;
        self.answer_sms_code(&code).await
    }

    /// Manda `verify_reply` a `to`. Per i test, e per rispondere a una richiesta sola.
    pub async fn reply_to(
        &mut self,
        to: &PublicKey,
        id: &str,
        calls: Vec<RingSeen>,
        code: Option<String>,
        unable: bool,
    ) -> bool {
        let Some(c) = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == to)
            .cloned()
        else {
            return false;
        };
        let app = AppMessage::VerifyReply {
            v: 1,
            id: id.to_string(),
            calls,
            code,
            unable,
        };
        // Sveglia chi verifica: sta aspettando l'esito.
        self.send_app_with(&c, &app, Kind::Normal, None)
            .await
            .is_ok()
    }

    pub(crate) fn received_verify_request(
        &mut self,
        sender: PublicKey,
        id: String,
        method: VerifyMethod,
    ) -> Vec<Event> {
        let t = now();
        self.state
            .verify_requests
            .retain(|r| r.received_at + VERIFY_TTL_SECS > t && r.id != id);
        // Poche alla volta: le richieste non costano niente a chi le manda.
        if self.state.verify_requests.len() >= 20 {
            return Vec::new();
        }
        self.state.verify_requests.push(VerifyRequest {
            id: id.clone(),
            from: sender,
            method,
            received_at: t,
        });
        vec![Event::VerifyRequest {
            contact: sender,
            id,
            method,
        }]
    }

    pub(crate) fn received_verify_reply(
        &mut self,
        sender: PublicKey,
        id: &str,
        calls: &[RingSeen],
        code: Option<&str>,
        unable: bool,
    ) -> Vec<Event> {
        // Vale solo dalla chiave che stavo verificando: un altro non può rispondere al suo posto.
        let Some(pos) = self
            .state
            .verifications
            .iter()
            .position(|v| v.id == id && v.contact == sender)
        else {
            return Vec::new();
        };
        // Un tentativo solo per verifica.
        let v = self.state.verifications.remove(pos);
        if unable && v.method == VerifyMethod::Ring {
            // Non sa ricevere lo squillo (iPhone, versione Play senza filtro): si passa all'SMS.
            if let Some(c) = self.contact_mut(&sender) {
                c.can_ring = Some(false);
            }
            return vec![Event::VerifyFallback {
                contact: sender,
                number: v.number,
            }];
        }
        let fresh = v.started_at + VERIFY_TTL_SECS > now();
        let ok = fresh
            && match v.method {
                VerifyMethod::Ring => match (&v.my_number, v.rang_at) {
                    (Some(mine), Some(rang)) => ring_matches(&v.id, mine, rang, calls),
                    _ => false,
                },
                VerifyMethod::Sms => code.is_some() && code == v.code.as_deref(),
            };
        if ok {
            if let Some(c) = self.contact_mut(&sender) {
                c.phone = Some(v.number.clone());
                c.phone_checked = Some(v.number.clone());
                if v.method == VerifyMethod::Ring {
                    c.can_ring = Some(true);
                }
            }
            // Col numero confermato, la chat di una chiave vecchia può passare a questa (§10).
            self.note_key_change(&sender);
        }
        vec![Event::PhoneChecked {
            contact: sender,
            ok,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codice_dall_sms() {
        let sms = "Rondine: Silvia ti chiede di confermare il tuo numero. Codice 01234567 rondine:verifica/01234567";
        assert_eq!(code_in(sms).as_deref(), Some("01234567"));
        assert_eq!(code_in("rondine:verifica/123"), None);
        assert_eq!(code_in("ciao"), None);
    }

    #[test]
    fn letture_del_numero_dal_registro() {
        assert_eq!(readings("3401112233"), ["+393401112233", "+3401112233"]);
        assert_eq!(readings("+393401112233"), ["+393401112233"]);
        // L'emulatore (e certi numeri esteri) senza il +.
        assert!(readings("15555215556").contains(&"+15555215556".to_string()));
    }

    #[test]
    fn squillo_dal_mio_numero_col_ritardo_della_rete() {
        let mine = "+393402223344";
        let seen = |at, n: &str| RingSeen {
            at,
            caller: caller_print("x", n),
        };
        // Arrivato 5 s dopo, dal mio numero: sì.
        assert!(ring_matches("x", mine, 100, &[seen(105, mine)]));
        // Dal numero giusto ma troppo tardi, o troppo presto: no.
        assert!(!ring_matches("x", mine, 100, &[seen(115, mine)]));
        assert!(!ring_matches("x", mine, 100, &[seen(95, mine)]));
        // All'ora giusta ma da un altro numero: no.
        assert!(!ring_matches("x", mine, 100, &[seen(104, "+393409999999")]));
        // Un'altra chiamata nello stesso momento non guasta; troppe sì.
        assert!(ring_matches(
            "x",
            mine,
            100,
            &[seen(103, "+393409999999"), seen(104, mine)]
        ));
        let many: Vec<RingSeen> = (0..11).map(|i| seen(100 + i, mine)).collect();
        assert!(!ring_matches("x", mine, 100, &many));
        // L'impronta è legata alla verifica: quella di un'altra non vale.
        let other = RingSeen {
            at: 104,
            caller: caller_print("y", mine),
        };
        assert!(!ring_matches("x", mine, 100, &[other]));
    }
}
