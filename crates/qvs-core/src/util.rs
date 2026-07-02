use rand::Rng;
use sha1::{Digest, Sha1};

#[must_use]
pub fn generate_peer_id() -> [u8; 20] {
    let mut id = [0u8; 20];
    let prefix = b"-QV0001-";
    id[..8].copy_from_slice(prefix);
    let mut rng = rand::thread_rng();
    rng.fill(&mut id[8..]);
    id
}

#[must_use]
pub fn generate_node_id() -> [u8; 20] {
    let mut id = [0u8; 20];
    let mut rng = rand::thread_rng();
    rng.fill(&mut id);
    id
}

#[must_use]
pub fn xor_distance(a: &[u8; 20], b: &[u8; 20]) -> [u8; 20] {
    let mut dist = [0u8; 20];
    for (i, d) in dist.iter_mut().enumerate() {
        *d = a[i] ^ b[i];
    }
    dist
}

#[must_use]
pub fn hex_encode(data: &[u8]) -> String {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(data.len() * 2);
    for b in data {
        out.push(char::from(HEX_CHARS[(b >> 4) as usize]));
        out.push(char::from(HEX_CHARS[(b & 0x0f) as usize]));
    }
    out
}

#[allow(clippy::missing_errors_doc)]
pub fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("hex string must have even length".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn current_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[must_use]
pub fn sha1_hash(data: &[u8]) -> [u8; 20] {
    let mut hasher = Sha1::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut hash = [0u8; 20];
    hash.copy_from_slice(&result);
    hash
}

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum SimpleBencode {
    Int(i64),
    Str(Vec<u8>),
    List(Vec<SimpleBencode>),
    Dict(BTreeMap<Vec<u8>, SimpleBencode>),
}

impl SimpleBencode {
    #[must_use]
    pub fn as_int(&self) -> Option<i64> {
        if let Self::Int(i) = self {
            Some(*i)
        } else {
            None
        }
    }

    #[must_use]
    pub fn as_str_bytes(&self) -> Option<&[u8]> {
        if let Self::Str(s) = self {
            Some(s)
        } else {
            None
        }
    }

    #[must_use]
    pub fn as_list(&self) -> Option<&Vec<SimpleBencode>> {
        if let Self::List(l) = self {
            Some(l)
        } else {
            None
        }
    }

    #[must_use]
    pub fn as_dict(&self) -> Option<&BTreeMap<Vec<u8>, SimpleBencode>> {
        if let Self::Dict(d) = self {
            Some(d)
        } else {
            None
        }
    }
}

#[allow(clippy::missing_errors_doc)]
pub fn simple_bencode_decode(data: &[u8]) -> Result<(SimpleBencode, &[u8]), String> {
    if data.is_empty() {
        return Err("empty data".into());
    }
    match data[0] {
        b'i' => {
            let end_rel = data[1..]
                .iter()
                .position(|&b| b == b'e')
                .ok_or("unterminated int")?;
            let s = std::str::from_utf8(&data[1..][..end_rel]).map_err(|_| "invalid int")?;
            let val: i64 = s.parse().map_err(|_| "bad int")?;
            Ok((SimpleBencode::Int(val), &data[2 + end_rel..]))
        }
        b'0'..=b'9' => {
            let colon = data.iter().position(|&b| b == b':').ok_or("no colon")?;
            let s = std::str::from_utf8(&data[..colon]).map_err(|_| "invalid len")?;
            let len: usize = s.parse().map_err(|_| "bad len")?;
            let start = colon + 1;
            if start + len > data.len() {
                return Err("truncated string".into());
            }
            Ok((
                SimpleBencode::Str(data[start..start + len].to_vec()),
                &data[start + len..],
            ))
        }
        b'l' => {
            let mut rest = &data[1..];
            let mut items = Vec::new();
            while !rest.is_empty() && rest[0] != b'e' {
                let (val, remaining) = simple_bencode_decode(rest)?;
                items.push(val);
                rest = remaining;
            }
            if rest.is_empty() {
                return Err("unterminated list".into());
            }
            Ok((SimpleBencode::List(items), &rest[1..]))
        }
        b'd' => {
            let mut rest = &data[1..];
            let mut dict = BTreeMap::new();
            while !rest.is_empty() && rest[0] != b'e' {
                let (key, remaining) = simple_bencode_decode(rest)?;
                if let SimpleBencode::Str(k) = key {
                    let (val, remaining) = simple_bencode_decode(remaining)?;
                    dict.insert(k, val);
                    rest = remaining;
                } else {
                    return Err("dict key not string".into());
                }
            }
            if rest.is_empty() {
                return Err("unterminated dict".into());
            }
            Ok((SimpleBencode::Dict(dict), &rest[1..]))
        }
        c => Err(format!("unexpected byte: {c}")),
    }
}

#[must_use]
pub fn parse_compact_peers(data: &[u8]) -> Vec<(Vec<u8>, u16)> {
    data.chunks_exact(6)
        .map(|chunk| {
            let port = u16::from_be_bytes([chunk[4], chunk[5]]);
            (chunk[..4].to_vec(), port)
        })
        .collect()
}

#[must_use]
pub fn parse_dict_peers(list: &[SimpleBencode]) -> Vec<(Vec<u8>, u16)> {
    list.iter()
        .filter_map(|entry| {
            let dict = entry.as_dict()?;
            let ip = dict.get(b"ip".as_ref())?.as_str_bytes()?.to_vec();
            let port = u16::try_from(dict.get(b"port".as_ref())?.as_int()?).ok()?;
            Some((ip, port))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_peer_id() {
        let id = generate_peer_id();
        assert_eq!(id.len(), 20);
        assert_eq!(&id[..8], b"-QV0001-");
    }

    #[test]
    fn test_generate_node_id() {
        let id1 = generate_node_id();
        let id2 = generate_node_id();
        assert_eq!(id1.len(), 20);
        assert_eq!(id2.len(), 20);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_xor_distance() {
        let a = [0xFF; 20];
        let b = [0x00; 20];
        let dist = xor_distance(&a, &b);
        assert_eq!(dist, [0xFF; 20]);
    }

    #[test]
    fn test_hex_roundtrip() {
        let data = b"hello";
        let encoded = hex_encode(data);
        let decoded = hex_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_current_time_millis() {
        let t = current_time_millis();
        assert!(t > 1_700_000_000_000u64);
    }

    #[test]
    fn test_sha1_hash() {
        let hash = sha1_hash(b"hello");
        assert_eq!(hash.len(), 20);
        let expected = hex_decode("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        assert_eq!(&hash[..], &expected[..]);
    }

    #[test]
    fn test_simple_bencode_int() {
        let data = b"i42e";
        let (val, rest) = simple_bencode_decode(data).unwrap();
        assert_eq!(val, SimpleBencode::Int(42));
        assert!(rest.is_empty());
    }

    #[test]
    fn test_simple_bencode_str() {
        let data = b"4:spam";
        let (val, _) = simple_bencode_decode(data).unwrap();
        assert_eq!(val, SimpleBencode::Str(b"spam".to_vec()));
    }

    #[test]
    fn test_simple_bencode_dict() {
        let data = b"d3:key3:vale";
        let (val, _) = simple_bencode_decode(data).unwrap();
        let dict = val.as_dict().unwrap();
        assert_eq!(
            dict.get(b"key".as_ref()).and_then(|v| v.as_str_bytes()),
            Some(&b"val"[..])
        );
    }

    #[test]
    fn test_parse_compact_peers() {
        let data = vec![192u8, 168, 1, 1, 0x21, 0xAD, 10, 0, 0, 1, 0x1A, 0xE1];
        let peers = parse_compact_peers(&data);
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].1, 8621);
        assert_eq!(peers[1].1, 6881);
    }

    #[test]
    fn test_parse_dict_peers() {
        use std::collections::BTreeMap;
        let mut d1 = BTreeMap::new();
        d1.insert(b"ip".to_vec(), SimpleBencode::Str(b"192.168.1.1".to_vec()));
        d1.insert(b"port".to_vec(), SimpleBencode::Int(8621));
        let mut d2 = BTreeMap::new();
        d2.insert(b"ip".to_vec(), SimpleBencode::Str(b"10.0.0.1".to_vec()));
        d2.insert(b"port".to_vec(), SimpleBencode::Int(6881));
        let list = vec![SimpleBencode::Dict(d1), SimpleBencode::Dict(d2)];
        let peers = parse_dict_peers(&list);
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].1, 8621);
    }
}
