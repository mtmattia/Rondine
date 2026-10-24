//! Gettoni anonimi per depositare (SPEC §14.4): VOPRF (RFC 9497) su ristretto255, con la
//! libreria `voprf`. Chi ha un permesso chiede al nodo dei gettoni "ciechi"; quando ne usa uno,
//! il nodo vede che è valido ma non a chi l'aveva dato, quindi non sa da quale contatto arriva
//! il deposito. La prova del VOPRF garantisce che il nodo usi la stessa chiave per tutti.
//!
//! La chiave cambia ogni `EPOCH_SECS`: un gettone vale nella sua epoca e nella successiva, e il
//! nodo ricorda quelli spesi solo per quel tempo.

use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use voprf::{
    BlindedElement, EvaluationElement, Group, Proof, Ristretto255, VoprfClient, VoprfServer,
};

use crate::{Error, Result, Signable, b64};

/// Durata di un'epoca dei gettoni.
pub const EPOCH_SECS: u64 = 30 * 24 * 3600;
/// Gettoni chiesti al massimo in una volta.
pub const MAX_ISSUE: usize = 20;

const KEY_INFO: &[u8] = b"rondine/v1/token-key";

pub fn epoch(t: u64) -> u64 {
    t / EPOCH_SECS
}

/// Quello che si gioca: la casella è dentro l'input, così un gettone vale per una casella sola.
fn input(inbox: &str, nonce: &[u8]) -> Vec<u8> {
    let mut v = b"rondine/v1/token|".to_vec();
    v.extend_from_slice(inbox.as_bytes());
    v.push(b'|');
    v.extend_from_slice(nonce);
    v
}

/// La chiave pubblica dei gettoni di un'epoca, pubblicata firmata in `/v1/info`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TokenConfig {
    pub v: u32,
    pub epoch: u64,
    /// Elemento ristretto255 (b64).
    pub public_key: String,
}

impl Signable for TokenConfig {
    const CONTEXT: &'static str = "rondine/v1/token-key";
}

/// Un gettone pronto da spendere.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub epoch: u64,
    /// 32 byte casuali (b64).
    pub nonce: String,
    /// Uscita del VOPRF (b64).
    pub output: String,
}

impl Token {
    /// Impronta per ricordarlo speso.
    pub fn spent_id(&self) -> String {
        b64::encode(
            &blake3::hash(format!("{}|{}", self.epoch, self.nonce).as_bytes()).as_bytes()[..16],
        )
    }
}

/// Chiave dei gettoni del nodo per un'epoca.
pub struct TokenKey {
    epoch: u64,
    server: VoprfServer<Ristretto255>,
}

/// Risposta del nodo a una richiesta di gettoni.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Issued {
    pub epoch: u64,
    pub evaluated: Vec<String>,
    pub proof: String,
}

impl TokenKey {
    pub fn for_epoch(node_seed: &[u8; 32], epoch: u64) -> Result<Self> {
        let mut h = blake3::Hasher::new_derive_key("rondine/v1/token-key");
        h.update(node_seed);
        h.update(&epoch.to_be_bytes());
        let seed = h.finalize();
        let server = VoprfServer::<Ristretto255>::new_from_seed(seed.as_bytes(), KEY_INFO)
            .map_err(|_| Error::Crypto)?;
        Ok(Self { epoch, server })
    }

    pub fn config(&self) -> TokenConfig {
        TokenConfig {
            v: 1,
            epoch: self.epoch,
            public_key: b64::encode(<Ristretto255 as Group>::serialize_elem(
                self.server.get_public_key(),
            )),
        }
    }

    /// Valuta (alla cieca) le richieste: non vede i gettoni che ne verranno.
    pub fn issue(&self, blinded: &[String]) -> Result<Issued> {
        if blinded.is_empty() || blinded.len() > MAX_ISSUE {
            return Err(Error::Invalid("numero di gettoni non valido"));
        }
        let elems = blinded
            .iter()
            .map(|b| {
                BlindedElement::<Ristretto255>::deserialize(&b64::decode(b)?)
                    .map_err(|_| Error::Crypto)
            })
            .collect::<Result<Vec<_>>>()?;
        let res = self
            .server
            .batch_blind_evaluate(&mut OsRng, &elems)
            .map_err(|_| Error::Crypto)?;
        Ok(Issued {
            epoch: self.epoch,
            evaluated: res
                .messages
                .iter()
                .map(|m| b64::encode(m.serialize()))
                .collect(),
            proof: b64::encode(res.proof.serialize()),
        })
    }

    /// Il gettone vale per questa casella e questa epoca? (Se è già stato speso lo dice il nodo.)
    pub fn check(&self, inbox: &str, token: &Token) -> bool {
        if token.epoch != self.epoch {
            return false;
        }
        let (Ok(nonce), Ok(output)) = (b64::decode(&token.nonce), b64::decode(&token.output))
        else {
            return false;
        };
        if nonce.len() != 32 {
            return false;
        }
        self.server
            .evaluate(&input(inbox, &nonce))
            .is_ok_and(|o| o.as_slice() == output.as_slice())
    }
}

/// Richiesta di gettoni in corso, sul telefono.
pub struct Request {
    inbox: String,
    nonces: Vec<[u8; 32]>,
    clients: Vec<VoprfClient<Ristretto255>>,
    /// Da mandare al nodo.
    pub blinded: Vec<String>,
}

impl Request {
    pub fn new(inbox: &str, n: usize) -> Result<Self> {
        use rand::RngCore;
        let n = n.clamp(1, MAX_ISSUE);
        let mut nonces = Vec::with_capacity(n);
        let mut clients = Vec::with_capacity(n);
        let mut blinded = Vec::with_capacity(n);
        for _ in 0..n {
            let mut nonce = [0u8; 32];
            OsRng.fill_bytes(&mut nonce);
            let r = VoprfClient::<Ristretto255>::blind(&input(inbox, &nonce), &mut OsRng)
                .map_err(|_| Error::Crypto)?;
            blinded.push(b64::encode(r.message.serialize()));
            nonces.push(nonce);
            clients.push(r.state);
        }
        Ok(Self {
            inbox: inbox.to_string(),
            nonces,
            clients,
            blinded,
        })
    }

    /// Toglie la cecità e controlla la prova contro la chiave pubblicata (`config`, già
    /// verificata con la firma del nodo).
    pub fn finalize(self, issued: &Issued, config: &TokenConfig) -> Result<Vec<Token>> {
        if issued.epoch != config.epoch || issued.evaluated.len() != self.clients.len() {
            return Err(Error::Invalid("risposta dei gettoni non valida"));
        }
        let pk = <Ristretto255 as Group>::deserialize_elem(&b64::decode(&config.public_key)?)
            .map_err(|_| Error::Crypto)?;
        let messages = issued
            .evaluated
            .iter()
            .map(|e| {
                EvaluationElement::<Ristretto255>::deserialize(&b64::decode(e)?)
                    .map_err(|_| Error::Crypto)
            })
            .collect::<Result<Vec<_>>>()?;
        let proof = Proof::<Ristretto255>::deserialize(&b64::decode(&issued.proof)?)
            .map_err(|_| Error::Crypto)?;
        let inputs: Vec<Vec<u8>> = self.nonces.iter().map(|n| input(&self.inbox, n)).collect();
        let outputs = VoprfClient::batch_finalize(&inputs, &self.clients, &messages, &proof, pk)
            .map_err(|_| Error::Crypto)?;
        let mut out = Vec::with_capacity(self.nonces.len());
        for (nonce, o) in self.nonces.iter().zip(outputs) {
            let o = o.map_err(|_| Error::Crypto)?;
            out.push(Token {
                epoch: config.epoch,
                nonce: b64::encode(nonce),
                output: b64::encode(o.as_slice()),
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gettoni_ciechi_validi_una_casella_una_epoca() -> Result<()> {
        let key = TokenKey::for_epoch(&[3u8; 32], 7)?;
        let cfg = key.config();
        // La chiave è la stessa a ogni avvio del nodo.
        assert_eq!(cfg, TokenKey::for_epoch(&[3u8; 32], 7)?.config());
        let req = Request::new("casella", 3)?;
        let issued = key.issue(&req.blinded)?;
        let tokens = req.finalize(&issued, &cfg)?;
        assert_eq!(tokens.len(), 3);
        assert!(tokens.iter().all(|t| key.check("casella", t)));
        // Non per un'altra casella, né con l'uscita cambiata, né in un'altra epoca.
        assert!(!key.check("altra", &tokens[0]));
        let mut bad = tokens[0].clone();
        bad.output = tokens[1].output.clone();
        assert!(!key.check("casella", &bad));
        assert!(!TokenKey::for_epoch(&[3u8; 32], 8)?.check("casella", &tokens[0]));
        Ok(())
    }

    #[test]
    fn la_prova_scopre_una_chiave_diversa() -> Result<()> {
        // Il nodo valuta con una chiave diversa da quella pubblicata (per riconoscere chi):
        // il telefono se ne accorge.
        let published = TokenKey::for_epoch(&[3u8; 32], 7)?;
        let other = TokenKey::for_epoch(&[4u8; 32], 7)?;
        let req = Request::new("casella", 2)?;
        let issued = other.issue(&req.blinded)?;
        assert!(req.finalize(&issued, &published.config()).is_err());
        Ok(())
    }
}
