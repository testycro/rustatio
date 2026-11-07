use serde::Serialize;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BencodeError {
    #[error("Failed to parse bencode: {0}")]
    ParseError(String),
    #[error("Invalid bencode structure: {0}")]
    InvalidStructure(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, BencodeError>;

/// Max list/dict nesting accepted by [`parse`] (deeper input overflows the stack).
pub const MAX_NESTING_DEPTH: usize = 64;

#[derive(Debug, PartialEq, Eq)]
enum ScanError {
    TooDeep,
    Malformed,
}

/// Index just past the bencoded value starting at `start` (iterative, no recursion).
fn value_end(data: &[u8], start: usize) -> std::result::Result<usize, ScanError> {
    let mut pos = start;
    let mut depth = 0usize;
    loop {
        match *data.get(pos).ok_or(ScanError::Malformed)? {
            b'i' => {
                let rest = data.get(pos + 1..).ok_or(ScanError::Malformed)?;
                let rel = rest.iter().position(|&b| b == b'e').ok_or(ScanError::Malformed)?;
                pos += rel + 2;
            }
            b'l' | b'd' => {
                depth += 1;
                if depth > MAX_NESTING_DEPTH {
                    return Err(ScanError::TooDeep);
                }
                pos += 1;
                continue;
            }
            b'e' => {
                if depth == 0 {
                    return Err(ScanError::Malformed);
                }
                depth -= 1;
                pos += 1;
            }
            b'0'..=b'9' => {
                let (len, content_start) = string_header(data, pos).ok_or(ScanError::Malformed)?;
                pos = content_start.checked_add(len).ok_or(ScanError::Malformed)?;
                if pos > data.len() {
                    return Err(ScanError::Malformed);
                }
            }
            _ => return Err(ScanError::Malformed),
        }
        if depth == 0 {
            return Ok(pos);
        }
    }
}

/// Parses the `<len>:` string header at `pos`: (length, content start).
fn string_header(data: &[u8], pos: usize) -> Option<(usize, usize)> {
    let rest = data.get(pos..)?;
    let colon = rest.iter().take(21).position(|&b| b == b':')?;
    let len = std::str::from_utf8(&rest[..colon]).ok()?.parse::<usize>().ok()?;
    Some((len, pos + colon + 1))
}

/// Raw bytes of the value of `key` in the top-level dictionary of `data`, as written.
pub fn dict_value_bytes<'a>(data: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    if data.first() != Some(&b'd') {
        return None;
    }
    let mut pos = 1;
    loop {
        if *data.get(pos)? == b'e' {
            return None;
        }
        let (key_len, key_start) = string_header(data, pos)?;
        let key_end = key_start.checked_add(key_len)?;
        let current_key = data.get(key_start..key_end)?;
        let value_stop = value_end(data, key_end).ok()?;
        if current_key == key {
            return data.get(key_end..value_stop);
        }
        pos = value_stop;
    }
}

/// Parse bencode data from bytes
pub fn parse(data: &[u8]) -> Result<serde_bencode::value::Value> {
    if value_end(data, 0) == Err(ScanError::TooDeep) {
        return Err(BencodeError::ParseError(format!(
            "nesting deeper than {MAX_NESTING_DEPTH} levels"
        )));
    }
    serde_bencode::from_bytes(data).map_err(|e| BencodeError::ParseError(e.to_string()))
}

/// Encode data to bencode format
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_bencode::to_bytes(value).map_err(|e| BencodeError::ParseError(e.to_string()))
}

/// Extract a string value from a bencode dictionary
#[allow(clippy::implicit_hasher)]
pub fn get_string(
    dict: &HashMap<Vec<u8>, serde_bencode::value::Value>,
    key: &str,
) -> Result<String> {
    dict.get(key.as_bytes())
        .and_then(|v| match v {
            serde_bencode::value::Value::Bytes(b) => Some(String::from_utf8_lossy(b).to_string()),
            _ => None,
        })
        .ok_or_else(|| BencodeError::InvalidStructure(format!("Missing or invalid key: {key}")))
}

/// Extract an integer value from a bencode dictionary
#[allow(clippy::implicit_hasher)]
pub fn get_int(dict: &HashMap<Vec<u8>, serde_bencode::value::Value>, key: &str) -> Result<i64> {
    dict.get(key.as_bytes())
        .and_then(|v| match v {
            serde_bencode::value::Value::Int(i) => Some(*i),
            _ => None,
        })
        .ok_or_else(|| BencodeError::InvalidStructure(format!("Missing or invalid key: {key}")))
}

/// Extract bytes value from a bencode dictionary
#[allow(clippy::implicit_hasher)]
pub fn get_bytes(
    dict: &HashMap<Vec<u8>, serde_bencode::value::Value>,
    key: &str,
) -> Result<Vec<u8>> {
    dict.get(key.as_bytes())
        .and_then(|v| match v {
            serde_bencode::value::Value::Bytes(b) => Some(b.clone()),
            _ => None,
        })
        .ok_or_else(|| BencodeError::InvalidStructure(format!("Missing or invalid key: {key}")))
}

/// Extract byte length from a bencode dictionary
#[allow(clippy::implicit_hasher)]
pub fn get_bytes_len(
    dict: &HashMap<Vec<u8>, serde_bencode::value::Value>,
    key: &str,
) -> Result<usize> {
    dict.get(key.as_bytes())
        .and_then(|v| match v {
            serde_bencode::value::Value::Bytes(b) => Some(b.len()),
            _ => None,
        })
        .ok_or_else(|| BencodeError::InvalidStructure(format!("Missing or invalid key: {key}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_bencode::value::Value;

    fn map(entries: Vec<(Vec<u8>, Value)>) -> HashMap<Vec<u8>, Value> {
        let mut dict = HashMap::new();
        for (key, value) in entries {
            dict.insert(key, value);
        }
        dict
    }

    #[test]
    fn test_parse_simple_string() -> Result<()> {
        let data = b"4:spam";
        let result = parse(data)?;
        assert!(matches!(result, Value::Bytes(ref b) if b == b"spam"));
        Ok(())
    }

    #[test]
    fn test_parse_integer() -> Result<()> {
        let data = b"i42e";
        let result = parse(data)?;
        assert!(matches!(result, Value::Int(42)));
        Ok(())
    }

    #[test]
    fn test_parse_invalid() {
        let result = parse(b"notbencode");
        assert!(matches!(result, Err(BencodeError::ParseError(_))));
    }

    #[test]
    fn test_encode_roundtrip() -> Result<()> {
        let value = Value::Dict(map(vec![
            (b"foo".to_vec(), Value::Bytes(b"bar".to_vec())),
            (b"num".to_vec(), Value::Int(7)),
        ]));
        let data = encode(&value)?;
        let decoded = parse(&data)?;

        assert_eq!(decoded, value);
        Ok(())
    }

    #[test]
    fn test_get_string_ok() -> Result<()> {
        let dict = map(vec![(b"name".to_vec(), Value::Bytes(b"test".to_vec()))]);
        let value = get_string(&dict, "name")?;
        assert_eq!(value, "test");
        Ok(())
    }

    #[test]
    fn test_get_string_missing() {
        let dict = HashMap::new();
        let result = get_string(&dict, "name");
        assert!(matches!(result, Err(BencodeError::InvalidStructure(_))));
    }

    #[test]
    fn test_get_string_wrong_type() {
        let dict = map(vec![(b"name".to_vec(), Value::Int(1))]);
        let result = get_string(&dict, "name");
        assert!(matches!(result, Err(BencodeError::InvalidStructure(_))));
    }

    #[test]
    fn test_get_int_ok() -> Result<()> {
        let dict = map(vec![(b"num".to_vec(), Value::Int(9))]);
        let value = get_int(&dict, "num")?;
        assert_eq!(value, 9);
        Ok(())
    }

    #[test]
    fn test_get_int_missing() {
        let dict = HashMap::new();
        let result = get_int(&dict, "num");
        assert!(matches!(result, Err(BencodeError::InvalidStructure(_))));
    }

    #[test]
    fn test_get_int_wrong_type() {
        let dict = map(vec![(b"num".to_vec(), Value::Bytes(b"x".to_vec()))]);
        let result = get_int(&dict, "num");
        assert!(matches!(result, Err(BencodeError::InvalidStructure(_))));
    }

    #[test]
    fn test_get_bytes_ok() -> Result<()> {
        let dict = map(vec![(b"data".to_vec(), Value::Bytes(b"abc".to_vec()))]);
        let value = get_bytes(&dict, "data")?;
        assert_eq!(value, b"abc".to_vec());
        Ok(())
    }

    #[test]
    fn test_get_bytes_missing() {
        let dict = HashMap::new();
        let result = get_bytes(&dict, "data");
        assert!(matches!(result, Err(BencodeError::InvalidStructure(_))));
    }

    #[test]
    fn test_get_bytes_wrong_type() {
        let dict = map(vec![(b"data".to_vec(), Value::Int(1))]);
        let result = get_bytes(&dict, "data");
        assert!(matches!(result, Err(BencodeError::InvalidStructure(_))));
    }

    #[test]
    fn test_get_bytes_len_ok() -> Result<()> {
        let dict = map(vec![(b"data".to_vec(), Value::Bytes(b"abcd".to_vec()))]);
        let value = get_bytes_len(&dict, "data")?;
        assert_eq!(value, 4);
        Ok(())
    }

    #[test]
    fn test_get_bytes_len_missing() {
        let dict = HashMap::new();
        let result = get_bytes_len(&dict, "data");
        assert!(matches!(result, Err(BencodeError::InvalidStructure(_))));
    }

    #[test]
    fn test_parse_rejects_deep_nesting_without_overflowing() {
        let mut data = vec![b'l'; 100_000];
        data.resize(data.len() + 100_000, b'e');
        assert!(matches!(parse(&data), Err(BencodeError::ParseError(_))));

        let mut ok = vec![b'l'; MAX_NESTING_DEPTH];
        ok.extend_from_slice(&[b'e'; MAX_NESTING_DEPTH]);
        assert!(parse(&ok).is_ok());
    }

    #[test]
    fn test_dict_value_bytes_returns_raw_span() {
        // "4:info" appears in a value before the real key; info keys are unsorted.
        let data = b"d7:comment6:4:info4:infod4:name1:a6:lengthi3ee3:zzzi1ee";
        assert_eq!(dict_value_bytes(data, b"info"), Some(&b"d4:name1:a6:lengthi3ee"[..]));
        assert_eq!(dict_value_bytes(data, b"zzz"), Some(&b"i1e"[..]));
        assert_eq!(dict_value_bytes(data, b"missing"), None);
        assert_eq!(dict_value_bytes(b"l4:infoe", b"info"), None);
        assert_eq!(dict_value_bytes(b"d4:info", b"info"), None);
    }

    #[test]
    fn test_get_bytes_len_wrong_type() {
        let dict = map(vec![(b"data".to_vec(), Value::Int(1))]);
        let result = get_bytes_len(&dict, "data");
        assert!(matches!(result, Err(BencodeError::InvalidStructure(_))));
    }
}
