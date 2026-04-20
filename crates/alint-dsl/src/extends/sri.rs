//! Subresource Integrity hashes for remote `extends:` entries.
//!
//! Format: `sha256-<64 lowercase hex chars>`. The prefix doubles
//! as the algorithm discriminator so future additions (sha384,
//! sha512) slot in without breaking existing configs.

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    Sha256,
}

impl Algorithm {
    pub fn output_bytes(self) -> usize {
        match self {
            Self::Sha256 => 32,
        }
    }

    pub fn prefix(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
        }
    }
}

/// An integrity hash parsed from a `#sha256-<hex>` URL fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sri {
    pub algorithm: Algorithm,
    pub bytes: Vec<u8>,
}

impl Sri {
    /// Parse from the `<algo>-<hex>` form that appears in URL
    /// fragments.
    pub fn parse(s: &str) -> Result<Self, SriError> {
        let (prefix, hex) = s
            .split_once('-')
            .ok_or_else(|| SriError::Malformed(s.to_string()))?;
        let algorithm = match prefix {
            "sha256" => Algorithm::Sha256,
            other => return Err(SriError::UnsupportedAlgorithm(other.to_string())),
        };
        let bytes = decode_hex(hex).map_err(|()| SriError::InvalidHex(hex.to_string()))?;
        if bytes.len() != algorithm.output_bytes() {
            return Err(SriError::WrongLength {
                algorithm: algorithm.prefix(),
                expected: algorithm.output_bytes(),
                got: bytes.len(),
            });
        }
        Ok(Self { algorithm, bytes })
    }

    /// Hash `data` under this SRI's algorithm and return an `Err`
    /// if it doesn't match.
    pub fn verify(&self, data: &[u8]) -> Result<(), SriError> {
        let actual = match self.algorithm {
            Algorithm::Sha256 => {
                let mut h = Sha256::new();
                h.update(data);
                h.finalize().to_vec()
            }
        };
        if actual == self.bytes {
            Ok(())
        } else {
            Err(SriError::Mismatch {
                expected: self.encoded(),
                actual: format!("{}-{}", self.algorithm.prefix(), encode_hex(&actual)),
            })
        }
    }

    /// Canonical string form — the same text consumed by `parse`.
    /// Safe to use as a filename on every supported platform.
    pub fn encoded(&self) -> String {
        format!("{}-{}", self.algorithm.prefix(), encode_hex(&self.bytes))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SriError {
    #[error("SRI is malformed (expected `<algo>-<hex>`): {0:?}")]
    Malformed(String),
    #[error("unsupported SRI algorithm: {0:?} (only `sha256` is supported in this build)")]
    UnsupportedAlgorithm(String),
    #[error("SRI hash is not valid hex: {0:?}")]
    InvalidHex(String),
    #[error("{algorithm} SRI hash should be {expected} bytes, got {got}")]
    WrongLength {
        algorithm: &'static str,
        expected: usize,
        got: usize,
    },
    #[error("SRI mismatch: expected {expected}, actual {actual}")]
    Mismatch { expected: String, actual: String },
}

fn decode_hex(s: &str) -> Result<Vec<u8>, ()> {
    if s.len() % 2 != 0 {
        return Err(());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    for chunk in bytes.chunks(2) {
        let h = hex_digit(chunk[0])?;
        let l = hex_digit(chunk[1])?;
        out.push((h << 4) | l);
    }
    Ok(out)
}

fn hex_digit(c: u8) -> Result<u8, ()> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(10 + c - b'a'),
        b'A'..=b'F' => Ok(10 + c - b'A'),
        _ => Err(()),
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(s, "{b:02x}").unwrap();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    // SHA-256 of the empty string.
    const EMPTY_SHA256: &str =
        "sha256-e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn parses_and_verifies_empty_string_hash() {
        let sri = Sri::parse(EMPTY_SHA256).unwrap();
        sri.verify(b"").unwrap();
        assert_eq!(sri.encoded(), EMPTY_SHA256);
    }

    #[test]
    fn detects_content_drift() {
        let sri = Sri::parse(EMPTY_SHA256).unwrap();
        let err = sri.verify(b"not empty").unwrap_err();
        assert!(matches!(err, SriError::Mismatch { .. }), "{err}");
    }

    #[test]
    fn rejects_malformed_inputs() {
        assert!(matches!(
            Sri::parse("completelyunformatted").unwrap_err(),
            SriError::Malformed(_)
        ));
        assert!(matches!(
            Sri::parse("md5-deadbeef").unwrap_err(),
            SriError::UnsupportedAlgorithm(_)
        ));
        assert!(matches!(
            Sri::parse("sha256-xyz").unwrap_err(),
            SriError::InvalidHex(_)
        ));
        assert!(matches!(
            Sri::parse("sha256-aa").unwrap_err(),
            SriError::WrongLength {
                algorithm: "sha256",
                expected: 32,
                got: 1,
            }
        ));
    }

    #[test]
    fn round_trips_through_encoded_form() {
        let original = EMPTY_SHA256;
        let sri = Sri::parse(original).unwrap();
        let encoded = sri.encoded();
        let reparsed = Sri::parse(&encoded).unwrap();
        assert_eq!(sri, reparsed);
    }

    #[test]
    fn case_insensitive_hex_input_normalizes_to_lowercase() {
        let upper = "sha256-E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855";
        let sri = Sri::parse(upper).unwrap();
        assert_eq!(
            sri.encoded(),
            "sha256-e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
