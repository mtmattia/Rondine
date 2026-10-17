//! Numero di sicurezza: 60 cifre uguali sui due telefoni (SPEC §10).

use crate::PublicKey;

pub fn safety_number(a: &PublicKey, b: &PublicKey) -> String {
    let (lo, hi) = if a.0 <= b.0 { (a, b) } else { (b, a) };
    let mut h = blake3::Hasher::new();
    h.update(b"rondine/v1/safety");
    h.update(&lo.0);
    h.update(&hi.0);
    let mut d = [0u8; 64];
    h.finalize_xof().fill(&mut d);
    d.as_chunks::<5>()
        .0
        .iter()
        .take(12)
        .map(|c| {
            let n = c.iter().fold(0u64, |acc, &x| (acc << 8) | u64::from(x));
            format!("{:05}", n % 100_000)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Identity;

    #[test]
    fn simmetrico_e_lungo_60_cifre() {
        let a = Identity::generate().public();
        let b = Identity::generate().public();
        let n = safety_number(&a, &b);
        assert_eq!(n, safety_number(&b, &a));
        assert_eq!(n.chars().filter(char::is_ascii_digit).count(), 60);
        assert_ne!(n, safety_number(&a, &Identity::generate().public()));
    }
}
