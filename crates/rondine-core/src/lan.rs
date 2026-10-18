//! Rete locale (SPEC §12): identificativo annunciato con mDNS.

use crate::{PublicKey, b64};

/// Tipo di servizio mDNS di Rondine.
pub const MDNS_SERVICE: &str = "_rondine._tcp.local.";

/// Giorno UNIX di un istante in secondi.
pub fn day(ts: u64) -> u64 {
    ts / 86_400
}

/// `lan_id` di una chiave in un giorno: cambia ogni giorno e lo riconosce solo chi conosce la
/// chiave (SPEC §12.1).
pub fn lan_id(key: &PublicKey, day: u64) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"rondine/v1/lan");
    h.update(&key.0);
    h.update(&day.to_be_bytes());
    b64::encode(&h.finalize().as_bytes()[..16])
}

/// Nome d'istanza mDNS: `r-<lan_id>`.
pub fn instance_name(key: &PublicKey, day: u64) -> String {
    format!("r-{}", lan_id(key, day))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Identity;

    #[test]
    fn lan_id_cambia_ogni_giorno_ed_e_corto() {
        let k = Identity::generate().public();
        assert_eq!(lan_id(&k, 20_000), lan_id(&k, 20_000));
        assert_ne!(lan_id(&k, 20_000), lan_id(&k, 20_001));
        assert_ne!(
            lan_id(&k, 20_000),
            lan_id(&Identity::generate().public(), 20_000)
        );
        // Un'etichetta DNS sta in 63 caratteri.
        assert!(instance_name(&k, 20_000).len() <= 63);
    }
}
