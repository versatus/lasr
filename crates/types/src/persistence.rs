#![allow(async_fn_in_trait)]
//! Persistent storage types used in the protocol, or for testing means.
//! The main export of this module is the `PersistenceStore` trait, which
//! is used for generalizing access to some storage either for production
//! or in-memory key-value storage, and works in tandem with `ractor::Actor`.
//!
//! TL;DR allows using `HashMap` for storage in tests.

/// Drop in replacement trait for subbing out storage types where
/// they would otherwise be inconvenient.
///
/// NOTE: It's probably best to use these methods as trait methods
/// to avoid collisions with the already implemented `get` & `put`.
///
/// ## Example:
/// ```rust,ignore
/// PersistenceStore::get(&store, key).await.unwrap();
/// ```
pub trait PersistenceStore: Clone {
    type Myself;
    type Key;
    type Value;
    type Error;

    async fn new() -> Result<Self::Myself, Self::Error>;

    async fn get(&self, key: Self::Key) -> Result<Option<Self::Value>, Self::Error>;

    async fn put(&self, key: Self::Key, val: Self::Value) -> Result<(), Self::Error>;
}

impl PersistenceStore for tikv_client::RawClient {
    type Myself = tikv_client::RawClient;
    type Key = tikv_client::Key;
    type Value = tikv_client::Value;
    type Error = tikv_client::Error;

    async fn new() -> Result<Self, Self::Error> {
        const TIKV_CLIENT_PD_ENDPOINT: &str = "127.0.0.1:2379";
        tikv_client::RawClient::new(vec![TIKV_CLIENT_PD_ENDPOINT]).await
    }

    async fn get(&self, key: Self::Key) -> Result<Option<Self::Value>, Self::Error> {
        tikv_client::RawClient::get(self, key).await
    }

    async fn put(&self, key: Self::Key, val: Self::Value) -> Result<(), Self::Error> {
        tikv_client::RawClient::put(self, key, val).await
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PersistenceStoreError {
    #[error("value not found in persistence store")]
    ValueNotFound,
}

/// About as close as it gets to persistence.. without persistence.
#[derive(Clone)]
pub struct MockPersistenceStore<K, V>(
    std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<K, V>>>,
)
where
    K: std::cmp::Eq + std::hash::Hash + Clone,
    V: Clone;

impl<K, V> PersistenceStore for MockPersistenceStore<K, V>
where
    K: std::cmp::Eq + std::hash::Hash + Clone,
    V: Clone,
{
    type Myself = MockPersistenceStore<K, V>;
    type Key = K;
    type Value = V;
    type Error = PersistenceStoreError;

    async fn new() -> Result<Self::Myself, Self::Error> {
        Ok(MockPersistenceStore(std::sync::Arc::new(
            tokio::sync::Mutex::new(std::collections::HashMap::new()),
        )))
    }

    async fn get(&self, key: Self::Key) -> Result<Option<Self::Value>, Self::Error> {
        let map = self.0.lock().await;
        match map.get(&key) {
            Some(value) => Ok(Some(value.clone())),
            None => Err(PersistenceStoreError::ValueNotFound),
        }
    }

    async fn put(&self, key: Self::Key, val: Self::Value) -> Result<(), Self::Error> {
        let mut map = self.0.lock().await;
        map.insert(key, val);
        Ok(())
    }
}
