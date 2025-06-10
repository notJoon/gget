use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use blake3;
use moka::future::Cache as MemCache;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{fs, sync::Mutex, time};

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON serialization/deserialization error: {0}")]
    // TODO: consider to use CBOR instead of JSON to reduce size
    Json(#[from] serde_json::Error),
}

/// Entry stored on disk
type Timestamp = u64;

#[derive(Serialize, Deserialize)]
pub struct CacheEntry {
    content: String,      // raw bytes of the value
    timestamp: Timestamp, // seconds since epoch
    ttl: u64,             // TTL in seconds
}

#[async_trait]
pub trait AsyncStorage: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<String>, CacheError>;
    async fn set(&self, key: &str, value: &str) -> Result<(), CacheError>;
    async fn cleanup(&self) -> Result<(), CacheError>;
}

#[derive(Clone)]
pub struct DiskStorage {
    cache_dir: PathBuf,
    default_ttl: u64,
    lock: Arc<Mutex<()>>,
}

impl DiskStorage {
    /// Creates a new [DiskStorage] instance in given directory with TTL win seconds
    pub fn new(cache_dir: PathBuf, ttl: Duration) -> Self {
        Self {
            cache_dir,
            default_ttl: ttl.as_secs(),
            lock: Arc::new(Mutex::new(())),
        }
    }

    /// Compute hash-based file path for a key
    fn entry_path(&self, key: &str) -> PathBuf {
        let hash = blake3::hash(key.as_bytes()).to_hex();
        let subdir = &hash[0..2]; // first 2 chars of hash
        self.cache_dir.join(subdir).join(format!("{}.json", hash))
    }

    /// Current timestamp sec since epoch
    fn now_ts() -> Timestamp {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}

#[async_trait]
impl AsyncStorage for DiskStorage {
    async fn get(&self, key: &str) -> Result<Option<String>, CacheError> {
        let path = self.entry_path(key);
        if !path.exists() {
            return Ok(None);
        }

        let data = fs::read_to_string(&path).await?;
        let entry: CacheEntry = serde_json::from_str(&data)?;
        // check TTL
        if Self::now_ts() >= entry.timestamp + entry.ttl {
            let _ = fs::remove_file(&path).await?;
            return Ok(None);
        }
        Ok(Some(entry.content))
    }

    async fn set(&self, key: &str, value: &str) -> Result<(), CacheError> {
        let path = self.entry_path(key);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).await?;
        }
        let entry = CacheEntry {
            content: value.to_string(),
            timestamp: Self::now_ts(),
            ttl: self.default_ttl,
        };
        let json = serde_json::to_string(&entry)?;
        fs::write(&path, json).await?;
        Ok(())
    }

    async fn cleanup(&self) -> Result<(), CacheError> {
        // must ensure single concurrent cleanup
        let _guard = self.lock.lock().await;
        let now = Self::now_ts();
        let mut dir_entries = fs::read_dir(&self.cache_dir).await?;
        while let Some(sub) = dir_entries.next_entry().await? {
            let mut files = fs::read_dir(sub.path()).await?;
            while let Some(file) = files.next_entry().await? {
                let path = file.path();
                if let Ok(data) = fs::read_to_string(&path).await {
                    if let Ok(entry) = serde_json::from_str::<CacheEntry>(&data) {
                        if now > entry.timestamp + entry.ttl {
                            let _ = fs::remove_file(&path).await;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

pub struct HybridCache {
    mem: MemCache<String, String>,
    storage: DiskStorage,
}

impl HybridCache {
    pub fn new(cache_dir: PathBuf, ttl: Duration, max_in_mem: u64) -> Self {
        let storage = DiskStorage::new(cache_dir.clone(), ttl);
        let st = storage.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(3600));
            loop {
                interval.tick().await;
                let _ = st.cleanup().await;
            }
        });

        Self {
            mem: MemCache::builder()
                .time_to_live(ttl)
                .max_capacity(max_in_mem)
                .build(),
            storage,
        }
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>, CacheError> {
        if let Some(v) = self.mem.get(key).await {
            return Ok(Some(v));
        }
        if let Some(v) = self.storage.get(key).await? {
            self.mem.insert(key.to_string(), v.clone()).await;
            return Ok(Some(v));
        }
        Ok(None)
    }

    pub async fn set(&self, key: &str, value: &str) -> Result<(), CacheError> {
        self.storage.set(key, value).await?;
        self.mem.insert(key.to_string(), value.to_string()).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_disk_storage_set_get() {
        let dir = tempdir().unwrap();
        let storage = DiskStorage::new(dir.path().to_path_buf(), Duration::from_secs(3600));
        let key = "test_key";
        let val = "value";
        assert_eq!(storage.get(key).await.unwrap(), None);
        storage.set(key, val).await.unwrap();
        let got = storage.get(key).await.unwrap();
        assert_eq!(got.as_deref(), Some(val));
    }

    #[tokio::test]
    async fn test_disk_storage_expiry() {
        let dir = tempdir().unwrap();
        let storage = DiskStorage::new(dir.path().to_path_buf(), Duration::from_secs(0));
        let key = "expire_key";
        let val = "value";
        storage.set(key, val).await.unwrap();
        assert_eq!(storage.get(key).await.unwrap(), None);
        let path = storage.entry_path(key);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn test_hybrid_cache_basic() {
        let dir = tempdir().unwrap();
        let cache = HybridCache::new(dir.path().to_path_buf(), Duration::from_secs(3600), 10);
        let key = "hybrid";
        let val = "hybrid_val";
        assert_eq!(cache.get(key).await.unwrap(), None);
        cache.set(key, val).await.unwrap();
        assert_eq!(cache.get(key).await.unwrap().as_deref(), Some(val));
        let cache2 = HybridCache::new(dir.path().to_path_buf(), Duration::from_secs(3600), 10);
        assert_eq!(cache2.get(key).await.unwrap().as_deref(), Some(val));
    }
}
