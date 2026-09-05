//! Execution capsule: input / plan / output hashes plus optional Ed25519.

#![forbid(unsafe_code)]

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capsule {
    pub input_hash: String,
    pub plan_hash: String,
    pub output_hash: String,
    pub engine_version: String,
    pub signature: Option<String>,
    pub verifying_key: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub command: String,
    pub targets: Vec<String>,
    pub font_fingerprint: String,
    pub engine_version: String,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

pub fn record(input: &[u8], plan: &Plan, output: &[u8], key: Option<&SigningKey>) -> Capsule {
    let plan_bytes = serde_json::to_vec(plan).unwrap_or_default();
    let mut cap = Capsule {
        input_hash: sha256_hex(input),
        plan_hash: sha256_hex(&plan_bytes),
        output_hash: sha256_hex(output),
        engine_version: ENGINE_VERSION.to_string(),
        signature: None,
        verifying_key: None,
    };
    if let Some(key) = key {
        let msg = canonical_message(&cap);
        let sig: Signature = key.sign(&msg);
        cap.signature = Some(hex::encode(sig.to_bytes()));
        cap.verifying_key = Some(hex::encode(key.verifying_key().to_bytes()));
    }
    cap
}

fn canonical_message(cap: &Capsule) -> Vec<u8> {
    let mut m = Vec::new();
    m.extend_from_slice(cap.input_hash.as_bytes());
    m.extend_from_slice(cap.plan_hash.as_bytes());
    m.extend_from_slice(cap.output_hash.as_bytes());
    m.extend_from_slice(cap.engine_version.as_bytes());
    m
}

pub fn verify(cap: &Capsule) -> bool {
    let (Some(sig_hex), Some(vk_hex)) = (&cap.signature, &cap.verifying_key) else {
        return cap.signature.is_none();
    };
    let Ok(sig_bytes) = hex::decode(sig_hex) else {
        return false;
    };
    let Ok(vk_bytes) = hex::decode(vk_hex) else {
        return false;
    };
    let Ok(sig) = Signature::from_slice(&sig_bytes) else {
        return false;
    };
    let Ok(vk_arr) = <[u8; 32]>::try_from(vk_bytes.as_slice()) else {
        return false;
    };
    let Ok(vk) = VerifyingKey::from_bytes(&vk_arr) else {
        return false;
    };
    vk.verify(&canonical_message(cap), &sig).is_ok()
}

/// Interfaces only — settlement / optional disclose / PQ signatures are out of v1.
pub trait Settlement {
    fn settle(&self, _capsule: &Capsule) -> Result<(), String> {
        Err("settlement is out of v1".into())
    }
}

pub trait OptionalDisclose {
    fn disclose(&self, _capsule: &Capsule) -> Result<(), String> {
        Err("optional disclose is out of v1".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    #[test]
    fn three_hashes_are_distinct_for_distinct_payloads() {
        let plan = Plan {
            command: "Convert".into(),
            targets: vec!["PdfA".into()],
            font_fingerprint: "abc".into(),
            engine_version: ENGINE_VERSION.into(),
        };
        let cap = record(b"input", &plan, b"output", None);
        assert_ne!(cap.input_hash, cap.plan_hash);
        assert_ne!(cap.plan_hash, cap.output_hash);
        assert_eq!(cap.input_hash.len(), 64);
        assert!(verify(&cap));
    }

    #[test]
    fn ed25519_roundtrip() {
        let rng = [7u8; 32];
        let key = SigningKey::from_bytes(&rng);
        let plan = Plan {
            command: "Convert".into(),
            targets: vec!["Html".into()],
            font_fingerprint: "f".into(),
            engine_version: ENGINE_VERSION.into(),
        };
        let cap = record(b"in", &plan, b"out", Some(&key));
        assert!(cap.signature.is_some());
        assert!(verify(&cap));
        let mut bad = cap.clone();
        bad.output_hash = sha256_hex(b"tamper");
        assert!(!verify(&bad));
    }
}
