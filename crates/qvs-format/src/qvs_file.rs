use std::collections::BTreeMap;

use qvs_core::{InfoHash, QvodError};

use crate::bencode::BencodeValue;

#[derive(Debug, Clone)]
pub struct QvsFile {
    pub info_hash: InfoHash,
    pub filename: String,
    pub file_size: u64,
    pub piece_length: u64,
    pub pieces: Vec<[u8; 20]>,
    pub trackers: Vec<String>,
    pub keyframe_index: Option<Vec<u8>>,
}

impl QvsFile {
    #[allow(clippy::missing_errors_doc, clippy::cast_possible_wrap)]
    pub fn encode(&self) -> Result<Vec<u8>, QvodError> {
        let mut dict = BTreeMap::new();

        dict.insert(
            b"info_hash".to_vec(),
            BencodeValue::Str(self.info_hash.0.to_vec()),
        );
        dict.insert(
            b"filename".to_vec(),
            BencodeValue::Str(self.filename.as_bytes().to_vec()),
        );
        dict.insert(
            b"file_size".to_vec(),
            BencodeValue::Int(self.file_size as i64),
        );
        dict.insert(
            b"piece_length".to_vec(),
            BencodeValue::Int(self.piece_length as i64),
        );

        let pieces_concat: Vec<u8> = self.pieces.iter().flat_map(|p| p.to_vec()).collect();
        dict.insert(b"pieces".to_vec(), BencodeValue::Str(pieces_concat));

        let trackers_list: Vec<BencodeValue> = self
            .trackers
            .iter()
            .map(|t| BencodeValue::Str(t.as_bytes().to_vec()))
            .collect();
        dict.insert(b"trackers".to_vec(), BencodeValue::List(trackers_list));

        if let Some(kf) = &self.keyframe_index {
            dict.insert(b"keyframe_index".to_vec(), BencodeValue::Str(kf.clone()));
        }

        Ok(BencodeValue::Dict(dict).encode())
    }

    #[allow(
        clippy::missing_errors_doc,
        clippy::cast_sign_loss,
        clippy::redundant_closure_for_method_calls
    )]
    pub fn decode(data: &[u8]) -> Result<Self, QvodError> {
        let (val, _) = BencodeValue::decode(data)?;
        let dict = val
            .as_dict()
            .ok_or_else(|| QvodError::Bencode("expected dict".into()))?;

        let info_hash_bytes = dict
            .get(b"info_hash".as_ref())
            .and_then(|v| v.as_str_bytes())
            .ok_or_else(|| QvodError::Bencode("missing info_hash".into()))?;
        if info_hash_bytes.len() != 20 {
            return Err(QvodError::Bencode("info_hash must be 20 bytes".into()));
        }
        let mut info_hash_arr = [0u8; 20];
        info_hash_arr.copy_from_slice(info_hash_bytes);
        let info_hash = InfoHash(info_hash_arr);

        let filename_bytes = dict
            .get(b"filename".as_ref())
            .and_then(|v| v.as_str_bytes())
            .ok_or_else(|| QvodError::Bencode("missing filename".into()))?;
        let filename = String::from_utf8(filename_bytes.to_vec())
            .map_err(|_| QvodError::Bencode("invalid filename utf-8".into()))?;

        let file_size =
            dict.get(b"file_size".as_ref())
                .and_then(|v| v.as_int())
                .ok_or_else(|| QvodError::Bencode("missing file_size".into()))? as u64;

        let piece_length = dict
            .get(b"piece_length".as_ref())
            .and_then(|v| v.as_int())
            .ok_or_else(|| QvodError::Bencode("missing piece_length".into()))?
            as u64;

        let pieces_bytes = dict
            .get(b"pieces".as_ref())
            .and_then(|v| v.as_str_bytes())
            .ok_or_else(|| QvodError::Bencode("missing pieces".into()))?;
        if pieces_bytes.len() % 20 != 0 {
            return Err(QvodError::Bencode(
                "pieces length not multiple of 20".into(),
            ));
        }
        let pieces: Vec<[u8; 20]> = pieces_bytes
            .chunks_exact(20)
            .map(|c| {
                let mut arr = [0u8; 20];
                arr.copy_from_slice(c);
                arr
            })
            .collect();

        let trackers = dict
            .get(b"trackers".as_ref())
            .and_then(|v| v.as_list())
            .map(|list| {
                list.iter()
                    .filter_map(|v| {
                        v.as_str_bytes()
                            .and_then(|b| String::from_utf8(b.to_vec()).ok())
                    })
                    .collect()
            })
            .unwrap_or_default();

        let keyframe_index = dict
            .get(b"keyframe_index".as_ref())
            .and_then(|v| v.as_str_bytes())
            .map(|b| b.to_vec());

        Ok(Self {
            info_hash,
            filename,
            file_size,
            piece_length,
            pieces,
            trackers,
            keyframe_index,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_qvs_file() -> QvsFile {
        QvsFile {
            info_hash: InfoHash([0u8; 20]),
            filename: "test.mp4".into(),
            file_size: 1024,
            piece_length: 256,
            pieces: vec![[1u8; 20], [2u8; 20]],
            trackers: vec!["http://tracker.example.com/announce".into()],
            keyframe_index: None,
        }
    }

    #[test]
    fn test_roundtrip() {
        let original = sample_qvs_file();
        let encoded = original.encode().unwrap();
        let decoded = QvsFile::decode(&encoded).unwrap();
        assert_eq!(decoded.info_hash.0, original.info_hash.0);
        assert_eq!(decoded.filename, original.filename);
        assert_eq!(decoded.file_size, original.file_size);
        assert_eq!(decoded.piece_length, original.piece_length);
        assert_eq!(decoded.pieces.len(), original.pieces.len());
        assert_eq!(decoded.trackers, original.trackers);
    }

    #[test]
    fn test_with_keyframe_index() {
        let mut qf = sample_qvs_file();
        qf.keyframe_index = Some(vec![0u8, 1, 2, 3]);
        let encoded = qf.encode().unwrap();
        let decoded = QvsFile::decode(&encoded).unwrap();
        assert_eq!(decoded.keyframe_index, Some(vec![0u8, 1, 2, 3]));
    }
}
