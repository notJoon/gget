use base64::{engine::general_purpose, Engine as _};
use reqwest::{Client, Error as ReqwestError};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

use crate::cache::{CacheError, HybridCache};
use crate::query::{RpcParams, RpcRequest, RpcResponse};
use crate::DEFAULT_RPC_ENDPOINT;

const MAX_ENTRIES: u64 = 1_000;
const TTL: u64 = 24 * 3600;

#[derive(Error, Debug)]
pub enum PackageManagerError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] ReqwestError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Base64 decoding error: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("RPC error: {0}")]
    Rpc(String),
    #[error("Failed to create target directory: {0}")]
    DirectoryCreation(String),
    #[error("Failed to get package files: {0}")]
    PackageFiles(String),
    #[error("Failed to get file content for {file}: {error}")]
    FileContent { file: String, error: String },
    #[error("Cache error: {0}")]
    Cache(#[from] CacheError),
}

pub struct PackageManager {
    rpc_endpoint: String,
    http_client: Client,
    cache: HybridCache,
}

impl PackageManager {
    /// Creates a new PackageManager instance
    pub fn new(rpc_endpoint: Option<String>, cache_dir: PathBuf) -> Self {
        let endpoint = rpc_endpoint.unwrap_or_else(|| DEFAULT_RPC_ENDPOINT.to_string());
        let http_client = Client::new();
        let cache = HybridCache::new(cache_dir, Duration::from_secs(TTL), MAX_ENTRIES);

        Self {
            rpc_endpoint: endpoint,
            http_client,
            cache,
        }
    }

    /// Returns the RPC endpoint
    pub fn rpc_endpoint(&self) -> &str {
        &self.rpc_endpoint
    }

    /// Downloads a package and its files to the target directory
    pub async fn download_package(
        &self,
        pkg_path: &str,
        target_dir: &Path,
    ) -> Result<(), PackageManagerError> {
        // Create target directory if it doesn't exist
        if !target_dir.exists() {
            fs::create_dir_all(target_dir)
                .map_err(|e| PackageManagerError::DirectoryCreation(e.to_string()))?;
        }

        let files_key = format!("files:{}", pkg_path);
        let files: Vec<String> = if let Some(raw) = self.cache.get(&files_key).await? {
            serde_json::from_str(&raw)?
        } else {
            let list = self
                .get_package_files(pkg_path)
                .await
                .map_err(|e| PackageManagerError::PackageFiles(e.to_string()))?;
            let serialized = serde_json::to_string(&list)?;
            self.cache.set(&files_key, &serialized).await?;
            list
        };

        // for each file, fetch content via cache or RPC
        for file in files {
            let trimmed = file.trim();
            if trimmed.is_empty() {
                continue;
            }
            let file_path = format!("{}/{}", pkg_path, trimmed);
            let content_key = format!("file:{}", file_path);
            let content = if let Some(raw) = self.cache.get(&content_key).await? {
                raw
            } else {
                let cnt = self.get_file_content(&file_path).await.map_err(|e| {
                    PackageManagerError::FileContent {
                        file: file.clone(),
                        error: e.to_string(),
                    }
                })?;
                self.cache.set(&content_key, &cnt).await?;
                cnt
            };

            // write to disk
            let target = target_dir.join(&file);
            if let Some(p) = target.parent() {
                fs::create_dir_all(p)?;
            }
            fs::write(&target, &content)?;
            println!("Downloaded: {}", target.display());
        }

        Ok(())
    }

    /// Retrieves the list of files in a package
    async fn get_package_files(&self, pkg_path: &str) -> Result<Vec<String>, PackageManagerError> {
        let encoded_path = general_purpose::STANDARD.encode(pkg_path.as_bytes());
        let data = self.query_rpc(&encoded_path).await?;

        // Decode the response data
        let decoded_data = general_purpose::STANDARD.decode(&data)?;
        let files_list = String::from_utf8_lossy(&decoded_data);

        // Split the file list and filter out empty strings
        let files: Vec<String> = files_list
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(files)
    }

    /// Retrieves the content of a specific file
    async fn get_file_content(&self, file_path: &str) -> Result<String, PackageManagerError> {
        let encoded_path = general_purpose::STANDARD.encode(file_path.as_bytes());
        let data = self.query_rpc(&encoded_path).await?;

        // Decode the response data
        let decoded_data = general_purpose::STANDARD.decode(&data)?;
        let content = String::from_utf8_lossy(&decoded_data).to_string();

        Ok(content)
    }

    /// Sends a query to the RPC endpoint
    async fn query_rpc(&self, data: &str) -> Result<String, PackageManagerError> {
        let request = RpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "abci_query".to_string(),
            params: RpcParams {
                path: "vm/qfile".to_string(),
                data: data.to_string(),
            },
        };

        let response = self
            .http_client
            .post(&self.rpc_endpoint)
            .json(&request)
            .send()
            .await?;

        let rpc_response: RpcResponse = response.json().await?;

        if let Some(error) = rpc_response.result.response.response_base.error {
            return Err(PackageManagerError::Rpc(format!("RPC error: {}", error)));
        }

        Ok(rpc_response.result.response.response_base.data)
    }
}
