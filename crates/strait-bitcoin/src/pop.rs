//! Hemi Proof-of-Proof (PoP) publications on Bitcoin.
//!
//! PoP miners anchor Hemi to Bitcoin by publishing *keystones* (every 25th Hemi
//! block) in Bitcoin transactions. Each publication is one `OP_RETURN` output:
//!
//! ```text
//! 6a 4c 50 | "HEMI" | L2KeystoneAbrev (76 bytes)
//! OP_RETURN OP_PUSHDATA1 80
//! ```
//!
//! Format from `hemilabs/heminetwork` (`hemi/pop/pop.go`, `hemi/hemi.go`), and
//! confirmed against mainnet: Bitcoin block 976,349 publishes keystone 5,757,700,
//! whose `ep_hash` / `parent_ep_hash` / `state_root` are prefixes of that Hemi
//! block's hash / parent hash / state root.
//!
//! Publications are not tied to the PoPPayoutsV2 reward contract: they appear on
//! Bitcoin whether or not payouts are active. A published keystone commits to the
//! keystone chain before it, so it anchors every Hemi block at or below its own.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use strait_core::error::{Result, StraitError};

/// `"HEMI"`: the publication magic.
const MAGIC: &[u8; 4] = b"HEMI";
/// Serialized `L2KeystoneAbrev` size.
const KEYSTONE_ABREV_SIZE: usize = 76;
/// `OP_RETURN OP_PUSHDATA1 0x50` followed by 80 data bytes.
const SCRIPT_PREFIX: [u8; 3] = [0x6a, 0x4c, 0x50];
const SCRIPT_LEN: usize = SCRIPT_PREFIX.len() + MAGIC.len() + KEYSTONE_ABREV_SIZE;

/// An abbreviated Hemi keystone, as published on Bitcoin (`hemi.L2KeystoneAbrev`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeystoneAbrev {
    pub version: u8,
    /// Ethereum (L1) origin block of the keystone.
    pub l1_block: u32,
    /// The Hemi block this keystone is for (a multiple of 25).
    pub l2_block: u32,
    /// First 11 bytes of the keystone block's parent hash.
    pub parent_ep_hash: [u8; 11],
    /// First 12 bytes of the previous keystone's block hash.
    pub prev_keystone_ep_hash: [u8; 12],
    pub state_root: [u8; 32],
    /// First 12 bytes of the keystone block's hash.
    pub ep_hash: [u8; 12],
}

impl KeystoneAbrev {
    fn from_bytes(b: &[u8; KEYSTONE_ABREV_SIZE]) -> Self {
        let mut k = Self {
            version: b[0],
            l1_block: u32::from_be_bytes([b[1], b[2], b[3], b[4]]),
            l2_block: u32::from_be_bytes([b[5], b[6], b[7], b[8]]),
            parent_ep_hash: [0; 11],
            prev_keystone_ep_hash: [0; 12],
            state_root: [0; 32],
            ep_hash: [0; 12],
        };
        k.parent_ep_hash.copy_from_slice(&b[9..20]);
        k.prev_keystone_ep_hash.copy_from_slice(&b[20..32]);
        k.state_root.copy_from_slice(&b[32..64]);
        k.ep_hash.copy_from_slice(&b[64..76]);
        k
    }

    /// The 76-byte serialized form, as published (inverse of parsing).
    pub fn to_bytes(&self) -> [u8; KEYSTONE_ABREV_SIZE] {
        let mut b = [0u8; KEYSTONE_ABREV_SIZE];
        b[0] = self.version;
        b[1..5].copy_from_slice(&self.l1_block.to_be_bytes());
        b[5..9].copy_from_slice(&self.l2_block.to_be_bytes());
        b[9..20].copy_from_slice(&self.parent_ep_hash);
        b[20..32].copy_from_slice(&self.prev_keystone_ep_hash);
        b[32..64].copy_from_slice(&self.state_root);
        b[64..76].copy_from_slice(&self.ep_hash);
        b
    }

    /// Parse the 76-byte serialized form (as stored by `to_bytes`).
    pub fn from_slice(b: &[u8]) -> Option<Self> {
        let arr: &[u8; KEYSTONE_ABREV_SIZE] = b.try_into().ok()?;
        Some(Self::from_bytes(arr))
    }

    /// True when this keystone matches the canonical Hemi block at `l2_block`,
    /// given that block's hash, parent hash and state root. A publication that
    /// fails this is forged or for a non-canonical block, and must not anchor.
    pub fn matches_hemi_block(&self, hash: &[u8; 32], parent_hash: &[u8; 32], state_root: &[u8; 32]) -> bool {
        hash[..12] == self.ep_hash
            && parent_hash[..11] == self.parent_ep_hash
            && *state_root == self.state_root
    }
}

/// Decode a PoP publication from an output script, or `None` if it isn't one.
pub fn parse_publication_script(script: &[u8]) -> Option<KeystoneAbrev> {
    if script.len() != SCRIPT_LEN || script[..3] != SCRIPT_PREFIX || &script[3..7] != MAGIC {
        return None;
    }
    let body: &[u8; KEYSTONE_ABREV_SIZE] = script[7..].try_into().ok()?;
    Some(KeystoneAbrev::from_bytes(body))
}

/// A confirmed transaction from a miner's history, with any PoP publications in it.
#[derive(Debug, Clone)]
pub struct MinerTx {
    /// Bitcoin txid, display (big-endian) hex.
    pub txid: String,
    pub block_height: u64,
    pub block_hash: String,
    pub block_time: DateTime<Utc>,
    /// `(vout, keystone)` for each PoP publication output. Usually one.
    pub publications: Vec<(u32, KeystoneAbrev)>,
}

#[derive(Deserialize)]
struct EsploraTx {
    txid: String,
    vout: Vec<EsploraVout>,
    status: EsploraStatus,
}

#[derive(Deserialize)]
struct EsploraVout {
    #[serde(default)]
    scriptpubkey: String,
}

#[derive(Deserialize)]
struct EsploraStatus {
    confirmed: bool,
    block_height: Option<u64>,
    block_hash: Option<String>,
    block_time: Option<i64>,
}

#[derive(Deserialize)]
struct EsploraBlock {
    timestamp: i64,
}

/// Minimal Esplora REST client (mempool.space, blockstream.info, or self-hosted).
pub struct EsploraClient {
    http: reqwest::Client,
    base: String,
}

impl EsploraClient {
    /// `base` is the API root, e.g. `https://mempool.space/api`.
    pub fn new(base: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .user_agent("strait-indexer")
            .build()
            .map_err(|e| StraitError::Http(format!("http client: {e}")))?;
        Ok(Self { http, base: base.trim_end_matches('/').to_string() })
    }

    async fn get(&self, path: &str) -> Result<reqwest::Response> {
        let url = format!("{}{}", self.base, path);
        let res = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| StraitError::Http(format!("esplora GET {path}: {e}")))?;
        if !res.status().is_success() {
            return Err(StraitError::Http(format!("esplora GET {path}: HTTP {}", res.status())));
        }
        Ok(res)
    }

    async fn get_text(&self, path: &str) -> Result<String> {
        self.get(path)
            .await?
            .text()
            .await
            .map(|s| s.trim().to_string())
            .map_err(|e| StraitError::Http(format!("esplora read {path}: {e}")))
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.get(path)
            .await?
            .json()
            .await
            .map_err(|e| StraitError::Parse(format!("esplora decode {path}: {e}")))
    }

    pub async fn tip_height(&self) -> Result<u64> {
        let s = self.get_text("/blocks/tip/height").await?;
        s.parse().map_err(|_| StraitError::Parse(format!("esplora tip height: {s:?}")))
    }

    /// Timestamp of the block at `height`.
    pub async fn block_time(&self, height: u64) -> Result<DateTime<Utc>> {
        let hash = self.get_text(&format!("/block-height/{height}")).await?;
        let b: EsploraBlock = self.get_json(&format!("/block/{hash}")).await?;
        DateTime::from_timestamp(b.timestamp, 0)
            .ok_or_else(|| StraitError::Parse(format!("block {height}: bad timestamp")))
    }

    /// One page (25 on mempool.space / Esplora) of an address's confirmed
    /// transactions, newest first. Pass the last txid of the previous page as
    /// `after` to continue. An empty page means the history is exhausted.
    pub async fn address_txs(&self, address: &str, after: Option<&str>) -> Result<Vec<MinerTx>> {
        let path = match after {
            Some(txid) => format!("/address/{address}/txs/chain/{txid}"),
            None => format!("/address/{address}/txs/chain"),
        };
        let page: Vec<EsploraTx> = self.get_json(&path).await?;
        let mut out = Vec::with_capacity(page.len());
        for tx in page {
            let (Some(height), Some(hash), Some(time), true) = (
                tx.status.block_height,
                tx.status.block_hash,
                tx.status.block_time,
                tx.status.confirmed,
            ) else {
                continue;
            };
            let block_time = DateTime::from_timestamp(time, 0)
                .ok_or_else(|| StraitError::Parse(format!("tx {}: bad block time", tx.txid)))?;
            let publications = tx
                .vout
                .iter()
                .enumerate()
                .filter_map(|(i, o)| {
                    let script = hex::decode(&o.scriptpubkey).ok()?;
                    parse_publication_script(&script).map(|k| (i as u32, k))
                })
                .collect();
            out.push(MinerTx { txid: tx.txid, block_height: height, block_hash: hash, block_time, publications });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real mainnet publication from Bitcoin block 976,349 (keystone 5,757,700).
    const SAMPLE: &str = "6a4c5048454d4901019369f30057db04d479a5cb96f86bcaa40a9e5ddc0cef956d5b3c797368a9a6c777eaadff541428ca43e8d58b9754edbd4c589f871d52d8187575e4699ee05b3c0db743ea443bdcd4bd68";

    fn h32(s: &str) -> [u8; 32] {
        hex::decode(s).unwrap().try_into().unwrap()
    }

    #[test]
    fn parses_real_mainnet_publication() {
        let k = parse_publication_script(&hex::decode(SAMPLE).unwrap()).expect("publication");
        assert_eq!(k.version, 1);
        assert_eq!(k.l1_block, 0x0193_69f3);
        assert_eq!(k.l2_block, 5_757_700);
        assert_eq!(k.l2_block % 25, 0);
        assert_eq!(hex::encode(k.ep_hash), "5b3c0db743ea443bdcd4bd68");
        // Round-trips through the stored form.
        assert_eq!(&k.to_bytes()[..], &hex::decode(SAMPLE).unwrap()[7..]);
        assert_eq!(KeystoneAbrev::from_slice(&k.to_bytes()), Some(k));
    }

    #[test]
    fn validates_against_the_real_hemi_block() {
        let k = parse_publication_script(&hex::decode(SAMPLE).unwrap()).unwrap();
        // Hemi mainnet block 5,757,700 (rpc.hemi.network).
        let hash = h32("5b3c0db743ea443bdcd4bd68490564ae9628b6a20f0c45d410a4f1de1f541daf");
        let parent = h32("d479a5cb96f86bcaa40a9eb18b6a81ad2003c5c2923e223277d6f8919c9c85ca");
        let root = h32("a6c777eaadff541428ca43e8d58b9754edbd4c589f871d52d8187575e4699ee0");
        assert!(k.matches_hemi_block(&hash, &parent, &root));

        let mut other = hash;
        other[0] ^= 1;
        assert!(!k.matches_hemi_block(&other, &parent, &root));
    }

    #[test]
    fn rejects_non_publications() {
        let mut s = hex::decode(SAMPLE).unwrap();
        assert!(parse_publication_script(&s[..s.len() - 1]).is_none()); // short
        s[3] = b'X'; // wrong magic
        assert!(parse_publication_script(&s).is_none());
        assert!(parse_publication_script(&[0x6a, 0x04, 0, 0, 3, 0x27]).is_none()); // tunnel payout OP_RETURN
    }
}
