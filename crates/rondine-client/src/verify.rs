//! Verifica del numero tra amici (SPEC §14.1): chi verifica chiama (squillo) o scrive (SMS) il
//! numero che ha in rubrica, e la prova torna dalla chiave da verificare nella chat cifrata. Una
//! chiave falsa data da qualcuno nel mezzo non riceve né squillo né SMS: suonano sulla SIM vera.
//! Il nodo non vede né numeri né codici.
//!
//! Le chiamate e gli SMS li fa il telefono (l'app Android); qui ci sono il piano, il confronto
//! e i messaggi nella chat.

use rondine_core::envelope::{AppMessage, VerifyMethod};
use rondine_core::{PublicKey, b64, now, phone, random_id};
use serde::{Deserialize, Serialize};

use crate::{Client, Error, Event, Kind, Result};

/// Ritardo accettato tra la partenza della chiamata e lo squillo dall'altra parte (rete mobile),
/// e scarto tra i due orologi di rete.
const LAG_SECS: std::ops::RangeInclusive<i64> = -2..=10;
/// Quanto può scostarsi la distanza tra due squilli vista dai due telefoni.
const TOLERANCE_SECS: i64 = 2;
/// Chiamate che fa chi verifica.
const RINGS: usize = 3;
/// Istanti accettati nella risposta: di più vorrebbe dire tirare a indovinare.
const MAX_TIMES: usize = 5;
/// Dopo quanto una verifica o una richiesta non vale più.
const VERIFY_TTL_SECS: u64 = 15 * 60;
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
    /// Quando sono partite le mie chiamate (squillo).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rings: Vec<u64>,
    /// Il codice mandato per SMS.
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
    /// Squillo: dopo quanti secondi da adesso far partire ogni chiamata (poi `record_ring`).
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

/// Un intero a caso tra `lo` e `hi` compresi.
fn uniform(lo: u64, hi: u64) -> Result<u64> {
    Ok(lo + random_u64()? % (hi - lo + 1))
}

/// Gli squilli tornano? Il primo arriva con un ritardo di rete (`LAG_SECS`), gli altri alla stessa
/// distanza dal primo che ha visto chi chiamava. Indovinarli a caso riesce circa una volta su
/// 1500 (SPEC §14.1).
fn rings_match(rings: &[u64], at: &[u64]) -> bool {
    if rings.len() < RINGS || at.len() < RINGS || at.len() > MAX_TIMES {
        return false;
    }
    let to_i64 = |v: &[u64]| {
        v.iter()
            .map(|&x| i64::try_from(x))
            .collect::<std::result::Result<Vec<_>, _>>()
    };
    let (Ok(rings), Ok(at)) = (to_i64(rings), to_i64(at)) else {
        return false;
    };
    let first = rings[0];
    at.iter().any(|&a0| {
        let lag = a0 - first;
        LAG_SECS.contains(&lag)
            && rings[1..]
                .iter()
                .all(|r| at.iter().any(|a| (a - r - lag).abs() <= TOLERANCE_SECS))
    })
}

/// Il codice in un SMS di verifica: le 8 cifre dopo il link.
fn code_in(text: &str) -> Option<String> {
    let rest = &text[text.find(SMS_LINK)? + SMS_LINK.len()..];
    let code: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    (code.len() == 8).then_some(code)
}

impl Client {
    /// Avvia la verifica del numero di un contatto. `number`: quello in rubrica, se il contatto
    /// non ce l'ha già. Avvisa il contatto nella chat (senza dire quando partono le chiamate) e
    /// restituisce il piano per il telefono.
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
        let (delays, code, sms_text) = match method {
            VerifyMethod::Ring => {
                let mut at = uniform(3, 60)?;
                let mut delays = vec![at];
                for _ in 1..RINGS {
                    at += uniform(10, 60)?;
                    delays.push(at);
                }
                (delays, None, None)
            }
            VerifyMethod::Sms => {
                let code = format!("{:08}", random_u64()? % 100_000_000);
                let text = format!(
                    "Rondine: {} ti chiede di confermare il tuo numero. Codice {code} {SMS_LINK}{code}",
                    self.state.display_name
                );
                (Vec::new(), Some(code), Some(text))
            }
        };
        let t = now();
        // Una verifica alla volta per contatto e metodo; le scadute si buttano.
        self.state.verifications.retain(|v| {
            v.started_at + VERIFY_TTL_SECS > t && !(v.contact == *contact && v.method == method)
        });
        self.state.verifications.push(Verification {
            id: id.clone(),
            contact: *contact,
            number: number.clone(),
            method,
            started_at: t,
            rings: Vec::new(),
            code: code.clone(),
        });
        self.save()?;
        let app = AppMessage::VerifyRequest {
            v: 1,
            id: id.clone(),
            method,
        };
        self.send_app_with(&c, &app, Kind::Quiet, None).await?;
        Ok(VerifyPlan {
            id,
            number,
            method,
            delays,
            code,
            sms_text,
        })
    }

    /// Il telefono ha fatto partire una chiamata della verifica `id` all'istante `at`.
    pub fn record_ring(&mut self, id: &str, at: u64) -> Result<()> {
        let v = self
            .state
            .verifications
            .iter_mut()
            .find(|v| v.id == id && v.method == VerifyMethod::Ring)
            .ok_or_else(|| Error::NotFound("verifica sconosciuta".into()))?;
        if v.rings.len() < RINGS {
            v.rings.push(at);
        }
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

    /// Risponde a uno squillo: gli istanti delle chiamate perse viste dal telefono.
    pub async fn answer_ring(&mut self, id: &str, at: &[u64]) -> Result<()> {
        let req = self
            .state
            .verify_requests
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or_else(|| Error::NotFound("richiesta di verifica sconosciuta".into()))?;
        self.answer_ring_to(&req.from, id, at).await?;
        self.state.verify_requests.retain(|r| r.id != id);
        self.save()
    }

    /// Manda a `to` gli istanti delle chiamate perse per la verifica `id`.
    pub async fn answer_ring_to(&mut self, to: &PublicKey, id: &str, at: &[u64]) -> Result<()> {
        let c = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == to)
            .cloned()
            .ok_or_else(|| Error::NotFound("contatto sconosciuto".into()))?;
        let app = AppMessage::VerifyReply {
            v: 1,
            id: id.to_string(),
            at: at.to_vec(),
            code: None,
        };
        self.send_app_with(&c, &app, Kind::Quiet, None).await?;
        Ok(())
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
            let Some(c) = self
                .state
                .contacts
                .iter()
                .find(|c| c.identity == r.from)
                .cloned()
            else {
                continue;
            };
            let app = AppMessage::VerifyReply {
                v: 1,
                id: r.id.clone(),
                at: Vec::new(),
                code: Some(code.clone()),
            };
            if self
                .send_app_with(&c, &app, Kind::Quiet, None)
                .await
                .is_ok()
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
        at: &[u64],
        code: Option<&str>,
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
        let fresh = v.started_at + VERIFY_TTL_SECS > now();
        let ok = fresh
            && match v.method {
                VerifyMethod::Ring => rings_match(&v.rings, at),
                VerifyMethod::Sms => code.is_some() && code == v.code.as_deref(),
            };
        if ok {
            if let Some(c) = self.contact_mut(&sender) {
                c.phone = Some(v.number.clone());
                c.phone_checked = Some(v.number.clone());
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
    fn squilli_col_ritardo_della_rete() {
        let r = [100, 130, 175];
        // Arrivati 5 s dopo, con le stesse distanze (±2 s): sì.
        assert!(rings_match(&r, &[105, 136, 179]));
        // Uno spostato di 4 s rispetto agli altri: no.
        assert!(!rings_match(&r, &[105, 139, 180]));
        // Troppo in ritardo, o troppo pochi, o troppi: no.
        assert!(!rings_match(&r, &[115, 145, 190]));
        assert!(!rings_match(&r, &[105, 135]));
        assert!(!rings_match(&r, &[1, 2, 3, 4, 5, 105]));
        // Un istante in più (un'altra chiamata nel frattempo) non guasta.
        assert!(rings_match(&r, &[90, 105, 135, 180]));
    }

    #[test]
    fn ritardi_casuali_nei_limiti() -> Result<()> {
        for _ in 0..200 {
            let d = uniform(3, 60)?;
            assert!((3..=60).contains(&d));
        }
        Ok(())
    }
}
