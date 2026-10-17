//! Permesso di consegna, emesso e firmato dal nodo (SPEC §5).

use serde::{Deserialize, Serialize};

use crate::{PublicKey, Signable, Signed};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Deposit,
    Wake,
    Keypackage,
}

impl Scope {
    pub const ALL: [Scope; 3] = [Scope::Deposit, Scope::Wake, Scope::Keypackage];
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Permit {
    pub v: u32,
    pub id: String,
    pub node_key: PublicKey,
    pub inbox: String,
    pub label: String,
    pub scopes: Vec<Scope>,
    pub issued_at: u64,
    pub expires_at: Option<u64>,
}

impl Permit {
    pub fn allows(&self, scope: Scope) -> bool {
        self.scopes.contains(&scope)
    }

    pub fn expired(&self, now: u64) -> bool {
        self.expires_at.is_some_and(|t| now >= t)
    }
}

impl Signable for Permit {
    const CONTEXT: &'static str = "rondine/v1/permit";
}

pub type SignedPermit = Signed<Permit>;
