//! STUN minimo (RFC 8489): solo *Binding*, senza autenticazione, per scoprire l'indirizzo visto
//! da fuori prima del buco nel NAT (SPEC §12.8). Il client fa la richiesta, il nodo risponde.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

const MAGIC_COOKIE: u32 = 0x2112_A442;
const BINDING_REQUEST: u16 = 0x0001;
const BINDING_SUCCESS: u16 = 0x0101;
const ATTR_MAPPED_ADDRESS: u16 = 0x0001;
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;
const HEADER_LEN: usize = 20;

pub type TransactionId = [u8; 12];

/// Identificativo casuale di una transazione.
pub fn transaction_id() -> TransactionId {
    use rand::RngCore;
    let mut t = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut t);
    t
}

fn header(kind: u16, len: u16, tx: &TransactionId) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + len as usize);
    out.extend_from_slice(&kind.to_be_bytes());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
    out.extend_from_slice(tx);
    out
}

/// Richiesta *Binding* senza attributi.
pub fn binding_request(tx: &TransactionId) -> Vec<u8> {
    header(BINDING_REQUEST, 0, tx)
}

/// Intestazione valida: tipo, lunghezza coerente e magic cookie. Restituisce tipo e transazione.
fn parse_header(msg: &[u8]) -> Option<(u16, TransactionId, &[u8])> {
    if msg.len() < HEADER_LEN {
        return None;
    }
    let kind = u16::from_be_bytes([msg[0], msg[1]]);
    let len = u16::from_be_bytes([msg[2], msg[3]]) as usize;
    let cookie = u32::from_be_bytes([msg[4], msg[5], msg[6], msg[7]]);
    // I primi due bit sono sempre zero in STUN; la lunghezza è multipla di 4.
    if kind & 0xC000 != 0 || cookie != MAGIC_COOKIE || !len.is_multiple_of(4) {
        return None;
    }
    let attrs = msg.get(HEADER_LEN..HEADER_LEN + len)?;
    let tx: TransactionId = msg[8..20].try_into().ok()?;
    Some((kind, tx, attrs))
}

/// Risposta del server a una richiesta *Binding*, con l'indirizzo da cui è arrivata.
/// `None` se il pacchetto non è una richiesta *Binding*.
pub fn answer(request: &[u8], from: SocketAddr) -> Option<Vec<u8>> {
    let (kind, tx, _) = parse_header(request)?;
    if kind != BINDING_REQUEST {
        return None;
    }
    let value = xor_address(from, &tx);
    let mut out = header(BINDING_SUCCESS, 4 + value.len() as u16, &tx);
    out.extend_from_slice(&ATTR_XOR_MAPPED_ADDRESS.to_be_bytes());
    out.extend_from_slice(&(value.len() as u16).to_be_bytes());
    out.extend_from_slice(&value);
    Some(out)
}

/// Valore di XOR-MAPPED-ADDRESS (lo XOR è simmetrico: serve per scrivere e per leggere).
fn xor_address(addr: SocketAddr, tx: &TransactionId) -> Vec<u8> {
    let port = addr.port() ^ (MAGIC_COOKIE >> 16) as u16;
    let mut mask = MAGIC_COOKIE.to_be_bytes().to_vec();
    mask.extend_from_slice(tx);
    let (family, ip): (u8, Vec<u8>) = match addr.ip() {
        IpAddr::V4(v4) => (1, v4.octets().to_vec()),
        IpAddr::V6(v6) => (2, v6.octets().to_vec()),
    };
    let mut out = vec![0, family];
    out.extend_from_slice(&port.to_be_bytes());
    out.extend(ip.iter().zip(mask.iter()).map(|(a, m)| a ^ m));
    out
}

fn read_address(value: &[u8], xor: Option<&TransactionId>) -> Option<SocketAddr> {
    let family = *value.get(1)?;
    let mut port = u16::from_be_bytes([*value.get(2)?, *value.get(3)?]);
    let raw = value.get(4..)?;
    let mut mask = MAGIC_COOKIE.to_be_bytes().to_vec();
    if let Some(tx) = xor {
        port ^= (MAGIC_COOKIE >> 16) as u16;
        mask.extend_from_slice(tx);
    } else {
        mask = vec![0; 16];
    }
    let bytes: Vec<u8> = raw.iter().zip(mask.iter()).map(|(a, m)| a ^ m).collect();
    let ip = match (family, bytes.len()) {
        (1, 4) => IpAddr::V4(Ipv4Addr::from(<[u8; 4]>::try_from(bytes).ok()?)),
        (2, 16) => IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(bytes).ok()?)),
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

/// Indirizzo visto dal server in una risposta *Binding* della transazione `tx`
/// (XOR-MAPPED-ADDRESS, o MAPPED-ADDRESS dei server vecchi).
pub fn mapped_address(response: &[u8], tx: &TransactionId) -> Option<SocketAddr> {
    let (kind, rtx, mut attrs) = parse_header(response)?;
    if kind != BINDING_SUCCESS || &rtx != tx {
        return None;
    }
    let mut plain = None;
    while attrs.len() >= 4 {
        let t = u16::from_be_bytes([attrs[0], attrs[1]]);
        let len = u16::from_be_bytes([attrs[2], attrs[3]]) as usize;
        let value = attrs.get(4..4 + len)?;
        match t {
            ATTR_XOR_MAPPED_ADDRESS => return read_address(value, Some(tx)),
            ATTR_MAPPED_ADDRESS => plain = read_address(value, None),
            _ => {}
        }
        // Gli attributi sono allineati a 4 byte.
        let padded = (4 + len).div_ceil(4) * 4;
        attrs = attrs.get(padded..).unwrap_or_default();
    }
    plain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indirizzo_come_nell_rfc_5769() -> Result<(), std::net::AddrParseError> {
        // Vettore di prova di RFC 5769 §2.2 (risposta IPv4): 192.0.2.1:32853.
        let tx: TransactionId = [
            0xb7, 0xe7, 0xa7, 0x01, 0xbc, 0x34, 0xd6, 0x86, 0xfa, 0x87, 0xdf, 0xae,
        ];
        let value = xor_address("192.0.2.1:32853".parse()?, &tx);
        assert_eq!(value, [0x00, 0x01, 0xa1, 0x47, 0xe1, 0x12, 0xa6, 0x43]);
        Ok(())
    }
}
