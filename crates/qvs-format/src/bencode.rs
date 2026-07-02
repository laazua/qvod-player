use std::collections::BTreeMap;

use qvs_core::QvodError;

#[derive(Debug, Clone, PartialEq)]
pub enum BencodeValue {
    Int(i64),
    Str(Vec<u8>),
    List(Vec<BencodeValue>),
    Dict(BTreeMap<Vec<u8>, BencodeValue>),
}

impl BencodeValue {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Int(i) => {
                let mut buf = Vec::new();
                buf.push(b'i');
                buf.extend_from_slice(i.to_string().as_bytes());
                buf.push(b'e');
                buf
            }
            Self::Str(s) => {
                let mut buf = Vec::new();
                buf.extend_from_slice(s.len().to_string().as_bytes());
                buf.push(b':');
                buf.extend_from_slice(s);
                buf
            }
            Self::List(items) => {
                let mut buf = Vec::new();
                buf.push(b'l');
                for item in items {
                    buf.extend_from_slice(&item.encode());
                }
                buf.push(b'e');
                buf
            }
            Self::Dict(entries) => {
                let mut buf = Vec::new();
                buf.push(b'd');
                for (key, val) in entries {
                    let key_bytes = Self::Str(key.clone());
                    buf.extend_from_slice(&key_bytes.encode());
                    buf.extend_from_slice(&val.encode());
                }
                buf.push(b'e');
                buf
            }
        }
    }

    #[allow(clippy::missing_errors_doc)]
    pub fn decode(data: &[u8]) -> Result<(Self, &[u8]), QvodError> {
        if data.is_empty() {
            return Err(QvodError::Bencode("empty data".into()));
        }
        match data[0] {
            b'i' => Self::decode_int(data),
            b'0'..=b'9' => Self::decode_str(data),
            b'l' => Self::decode_list(data),
            b'd' => Self::decode_dict(data),
            c => Err(QvodError::Bencode(format!("unexpected byte: 0x{c:02x}"))),
        }
    }

    fn decode_int(data: &[u8]) -> Result<(Self, &[u8]), QvodError> {
        if data.is_empty() || data[0] != b'i' {
            return Err(QvodError::Bencode("expected integer".into()));
        }
        let end = data[1..]
            .iter()
            .position(|&b| b == b'e')
            .map(|p| p + 1)
            .ok_or_else(|| QvodError::Bencode("unterminated integer".into()))?;
        let num_str = std::str::from_utf8(&data[1..end])
            .map_err(|_| QvodError::Bencode("invalid integer utf-8".into()))?;
        let val: i64 = num_str
            .parse()
            .map_err(|e| QvodError::Bencode(format!("invalid integer: {e}")))?;
        Ok((Self::Int(val), &data[end + 1..]))
    }

    fn decode_str(data: &[u8]) -> Result<(Self, &[u8]), QvodError> {
        let colon = data[..]
            .iter()
            .position(|&b| b == b':')
            .ok_or_else(|| QvodError::Bencode("unterminated string length".into()))?;
        let len_str = std::str::from_utf8(&data[..colon])
            .map_err(|_| QvodError::Bencode("invalid string length utf-8".into()))?;
        let len: usize = len_str
            .parse()
            .map_err(|e| QvodError::Bencode(format!("invalid string length: {e}")))?;
        let start = colon + 1;
        if start + len > data.len() {
            return Err(QvodError::Bencode("string data truncated".into()));
        }
        Ok((
            Self::Str(data[start..start + len].to_vec()),
            &data[start + len..],
        ))
    }

    fn decode_list(data: &[u8]) -> Result<(Self, &[u8]), QvodError> {
        if data.is_empty() || data[0] != b'l' {
            return Err(QvodError::Bencode("expected list".into()));
        }
        let mut rest = &data[1..];
        let mut items = Vec::new();
        while !rest.is_empty() && rest[0] != b'e' {
            let (val, remaining) = Self::decode(rest)?;
            items.push(val);
            rest = remaining;
        }
        if rest.is_empty() {
            return Err(QvodError::Bencode("unterminated list".into()));
        }
        Ok((Self::List(items), &rest[1..]))
    }

    fn decode_dict(data: &[u8]) -> Result<(Self, &[u8]), QvodError> {
        if data.is_empty() || data[0] != b'd' {
            return Err(QvodError::Bencode("expected dict".into()));
        }
        let mut rest = &data[1..];
        let mut dict = BTreeMap::new();
        while !rest.is_empty() && rest[0] != b'e' {
            let (key, remaining) = Self::decode_str(rest)?;
            let (val, remaining) = Self::decode(remaining)?;
            if let Self::Str(key_bytes) = key {
                dict.insert(key_bytes, val);
            }
            rest = remaining;
        }
        if rest.is_empty() {
            return Err(QvodError::Bencode("unterminated dict".into()));
        }
        Ok((Self::Dict(dict), &rest[1..]))
    }

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
    pub fn as_str(&self) -> Option<&str> {
        self.as_str_bytes()
            .and_then(|s| std::str::from_utf8(s).ok())
    }

    #[must_use]
    pub fn as_list(&self) -> Option<&Vec<BencodeValue>> {
        if let Self::List(l) = self {
            Some(l)
        } else {
            None
        }
    }

    #[must_use]
    pub fn as_dict(&self) -> Option<&BTreeMap<Vec<u8>, BencodeValue>> {
        if let Self::Dict(d) = self {
            Some(d)
        } else {
            None
        }
    }

    #[must_use]
    pub fn dict_get(&self, key: &[u8]) -> Option<&BencodeValue> {
        self.as_dict()?.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_int() {
        assert_eq!(BencodeValue::Int(42).encode(), b"i42e");
        assert_eq!(BencodeValue::Int(-1).encode(), b"i-1e");
    }

    #[test]
    fn test_encode_str() {
        assert_eq!(BencodeValue::Str(b"spam".to_vec()).encode(), b"4:spam");
    }

    #[test]
    fn test_encode_list() {
        let list = BencodeValue::List(vec![BencodeValue::Str(b"a".to_vec()), BencodeValue::Int(1)]);
        assert_eq!(list.encode(), b"l1:ai1ee");
    }

    #[test]
    fn test_encode_dict() {
        let mut dict = BTreeMap::new();
        dict.insert(b"key".to_vec(), BencodeValue::Str(b"val".to_vec()));
        let encoded = BencodeValue::Dict(dict).encode();
        assert_eq!(encoded, b"d3:key3:vale");
    }

    #[test]
    fn test_roundtrip_int() {
        let val = BencodeValue::Int(12345);
        let encoded = val.encode();
        let (decoded, rest) = BencodeValue::decode(&encoded).unwrap();
        assert_eq!(decoded, val);
        assert!(rest.is_empty());
    }

    #[test]
    fn test_roundtrip_str() {
        let val = BencodeValue::Str(b"hello world".to_vec());
        let encoded = val.encode();
        let (decoded, _) = BencodeValue::decode(&encoded).unwrap();
        assert_eq!(decoded, val);
    }

    #[test]
    fn test_roundtrip_nested() {
        let mut dict = BTreeMap::new();
        dict.insert(b"pieces".to_vec(), BencodeValue::Str(vec![0u8; 20]));
        dict.insert(b"name".to_vec(), BencodeValue::Str(b"test.txt".to_vec()));
        dict.insert(b"length".to_vec(), BencodeValue::Int(1024));
        let val = BencodeValue::Dict(dict);
        let encoded = val.encode();
        let (decoded, _) = BencodeValue::decode(&encoded).unwrap();
        assert_eq!(decoded, val);
    }

    #[test]
    fn test_dict_get() {
        let mut dict = BTreeMap::new();
        dict.insert(b"key1".to_vec(), BencodeValue::Int(42));
        let val = BencodeValue::Dict(dict);
        assert_eq!(val.dict_get(b"key1").and_then(|v| v.as_int()), Some(42));
        assert!(val.dict_get(b"nonexistent").is_none());
    }

    #[test]
    fn test_decode_tracker_response() {
        let mut dict = BTreeMap::new();
        dict.insert(b"interval".to_vec(), BencodeValue::Int(1800));
        dict.insert(b"complete".to_vec(), BencodeValue::Int(42));
        dict.insert(b"incomplete".to_vec(), BencodeValue::Int(17));
        dict.insert(b"peers".to_vec(), BencodeValue::Str(vec![0u8; 0]));
        let val = BencodeValue::Dict(dict);
        let encoded = val.encode();
        let (decoded, _) = BencodeValue::decode(&encoded).unwrap();
        let d = decoded.as_dict().unwrap();
        assert_eq!(
            d.get(b"interval".as_ref()).and_then(|v| v.as_int()),
            Some(1800)
        );
        assert_eq!(
            d.get(b"complete".as_ref()).and_then(|v| v.as_int()),
            Some(42)
        );
    }
}
