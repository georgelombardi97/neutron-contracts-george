use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::Binary;

/// Encodes bytes slice into hex string
pub fn encode_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// Decodes hex string into bytes vec
pub fn decode_hex(s: &str) -> Option<Vec<u8>> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RegisteredQuery {
    /// The unique id of the registered query.
    pub id: u64,
    /// The address that registered the query.
    pub owner: String,
    /// The KV-storage keys for which we want to get values from remote chain
    pub keys: Vec<KVKey>,
    /// The query type identifier (i.e. 'kv' or 'tx' for now)
    pub query_type: String,
    /// The filter for transaction search ICQ
    pub transactions_filter: String,
    /// The chain of interest identifier.
    pub zone_id: String,
    /// The IBC connection ID for getting ConsensusState to verify proofs.
    pub connection_id: String,
    /// Parameter that defines how often the query must be updated.
    pub update_period: u64,
    /// The local height when the event to update the query result was emitted last time.
    pub last_emitted_height: u64,
    /// The local chain last block height when the query result was updated.
    #[serde(default)]
    pub last_submitted_result_local_height: u64,
    /// The remote chain last block height when the query result was updated.
    #[serde(default)]
    pub last_submitted_result_remote_height: u64,
}

/// InterchainQueryResult is a result data for a registered query
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct InterchainQueryResult {
    /// **kv_results** is a raw key-value pairs of query result
    pub kv_results: Vec<StorageValue>,

    /// **height** is a height of remote chain
    pub height: u64,

    #[serde(default)]
    /// **revision** is a revision of remote chain
    pub revision: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
/// Describes value in the Cosmos-SDK KV-storage on remote chain
pub struct StorageValue {
    /// **storage_prefix** is a path to the storage (storage prefix) where you want to read value by key (usually name of cosmos-sdk module: 'staking', 'bank', etc.)
    pub storage_prefix: String,

    /// **key** is a key under which the **value** is stored in the storage on remote chain
    pub key: Binary,

    /// **value** is a value which is stored under the **key** in the storage on remote chain
    pub value: Binary,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
/// Type for wrapping any protobuf message
pub struct ProtobufAny {
    /// **type_url** describes the type of the serialized message
    pub type_url: String,

    ///  **value** must be a valid serialized protocol buffer of the above specified type
    pub value: Binary,
}

impl ProtobufAny {
    /// Helper to create new ProtobufAny type:
    /// * **type_url** describes the type of the serialized message
    /// * **value** must be a valid serialized protocol buffer of the above specified type
    pub fn new(type_url: String, value: Binary) -> Self {
        ProtobufAny { type_url, value }
    }
}

const KV_PATH_KEY_DELIMITER: &str = "/";
const KV_KEYS_DELIMITER: &str = ",";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
/// Describes a KV key for which you want to get value from the storage on remote chain
pub struct KVKey {
    /// **path** is a path to the storage (storage prefix) where you want to read value by key (usually name of cosmos-sdk module: 'staking', 'bank', etc.)
    pub path: String,

    /// **key** is a key you want to read from the storage
    pub key: Binary,
}

impl KVKey {
    /// Creates KVKey from string
    /// Returns None on failure
    pub fn from_string<S: AsRef<str>>(s: S) -> Option<KVKey> {
        let split: Vec<&str> = s.as_ref().split(KV_PATH_KEY_DELIMITER).collect();
        if split.len() < 2 {
            return None;
        }

        Some(KVKey {
            path: split[0].to_string(),
            key: Binary(decode_hex(split[1])?),
        })
    }
}

#[allow(clippy::from_over_into)]
impl Into<String> for &KVKey {
    fn into(self) -> String {
        let mut s = String::with_capacity(
            self.path.len() + KV_PATH_KEY_DELIMITER.len() + self.key.len() * 2,
        );

        s.push_str(&self.path);
        s.push_str(KV_PATH_KEY_DELIMITER);
        s.push_str(&encode_hex(&self.key));

        s
    }
}

/// KVKeys describes vec of KVKey structures
pub struct KVKeys(pub Vec<KVKey>);

impl KVKeys {
    /// Creates KVKeys from string
    /// Returns None on failure
    pub fn from_string<S: AsRef<str>>(s: S) -> Option<KVKeys> {
        let split = s.as_ref().split(KV_KEYS_DELIMITER);

        Some(KVKeys(
            split
                .map(KVKey::from_string)
                .collect::<Option<Vec<KVKey>>>()?,
        ))
    }
}

#[allow(clippy::from_over_into)]
impl Into<String> for KVKeys {
    fn into(self) -> String {
        self.0
            .iter()
            .map(|kv| kv.into())
            .collect::<Vec<String>>()
            .join(KV_KEYS_DELIMITER)
    }
}
