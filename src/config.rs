use std::{
    collections::HashMap,
    path::{Component, PathBuf},
};

use serde::{de::Error, Deserialize, Deserializer};
use serde_json::Value;

/// Format of the `diridp.yaml` file.
#[derive(Deserialize)]
pub struct Top {
    #[serde(default = "default_state_dir")]
    pub state_dir: PathBuf,
    pub providers: HashMap<String, Provider>,
}

#[derive(Deserialize)]
pub struct Provider {
    pub issuer: String,

    pub keys_dir: Option<PathBuf>,
    #[serde(default = "default_provider_key_lifespan")]
    pub key_lifespan: u64,
    pub key_publish_margin: Option<u64>,

    pub webroot: Option<PathBuf>,
    #[serde(deserialize_with = "deserialize_web_path")]
    #[serde(default = "default_provider_jwks_path")]
    pub jwks_path: String,
    pub jwks_uri: Option<String>,

    #[serde(default)]
    pub claims: HashMap<String, Value>,

    #[serde(default)]
    pub tokens: Vec<Token>,
}

#[derive(Deserialize)]
pub struct Token {
    pub path: TokenPath,
    #[serde(default = "default_token_lifespan")]
    pub lifespan: u64,
    pub refresh: Option<u64>,
    #[serde(default = "default_token_nbf_margin")]
    pub nbf_margin: u64,
    #[serde(default)]
    pub claims: HashMap<String, Value>,
}

pub enum TokenPath {
    SingleFile {
        path: PathBuf,
    },
    Directories {
        parent_dir: PathBuf,
        claim_name: String,
        filename: String,
    },
}

fn default_state_dir() -> PathBuf {
    "/var/lib/diridp".into()
}

fn default_provider_key_lifespan() -> u64 {
    86400
}

fn default_provider_jwks_path() -> String {
    "/jwks.json".into()
}

fn default_token_lifespan() -> u64 {
    3600
}

fn default_token_nbf_margin() -> u64 {
    5
}

/// Deserialize an absolute path inside the webroot, and deny special components.
fn deserialize_web_path<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let path = PathBuf::deserialize(deserializer)?;
    let mut parts = path.components();
    if parts.next() != Some(Component::RootDir) {
        return Err(D::Error::custom(
            "path must be an absolute path inside the webroot",
        ));
    }
    for part in parts {
        if !matches!(part, Component::Normal(_)) {
            return Err(D::Error::custom(
                "path must not contain relative components",
            ));
        }
    }
    let path = path
        .to_str()
        .ok_or_else(|| D::Error::custom("path must be valid UTF-8"))?;
    Ok(path.to_string())
}

impl<'de> Deserialize<'de> for TokenPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let path = PathBuf::deserialize(deserializer)?;
        let parts: Vec<_> = path
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();

        let mut indices = parts
            .iter()
            .enumerate()
            .filter(|(_, part)| part.starts_with(':'))
            .map(|(idx, _)| idx);
        if let Some(idx) = indices.next() {
            let expect_idx = parts.len() - 2;
            if idx != expect_idx || indices.next().is_some() {
                return Err(D::Error::custom(
                    "only one parameter, in the next-to-last position, is supported",
                ));
            }

            Ok(TokenPath::Directories {
                parent_dir: parts[..idx].iter().collect(),
                claim_name: parts[idx][1..].to_string(),
                filename: parts.last().unwrap().to_string(),
            })
        } else {
            Ok(TokenPath::SingleFile { path })
        }
    }
}
