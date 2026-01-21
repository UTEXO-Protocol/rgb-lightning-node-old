use crate::entities::{channel_peer_data, mnemonic, prelude::*, rgb_config};
use crate::error::APIError;
use bitcoin::secp256k1::PublicKey;
use migration::MigratorTrait;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, Database, DatabaseConnection, DeleteResult,
    EntityTrait, QueryFilter,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::fs;

pub struct DatabaseManager {
    db: DatabaseConnection,
    // Cache for RGB config to reduce database hits on frequent operations
    rgb_config_cache: Arc<Mutex<HashMap<String, Option<String>>>>,
}

impl DatabaseManager {
    pub async fn new(db_path: &Path) -> Result<Self, APIError> {
        tracing::info!("Initializing database at path: {}", db_path.display());
        let db_url = format!("sqlite://{}?mode=rwc", db_path.display());
        tracing::info!("Connecting to database URL: {}", db_url);
        let db = Database::connect(&db_url)
            .await
            .map_err(|e| APIError::DatabaseError(e.to_string()))?;
        tracing::info!("Database connected successfully");

        tracing::info!("Running migrations");
        migration::Migrator::up(&db, None)
            .await
            .map_err(|e| APIError::DatabaseError(e.to_string()))?;
        tracing::info!("Migrations completed");

        Ok(Self {
            db,
            rgb_config_cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn save_mnemonic(&self, encrypted_mnemonic: String) -> Result<(), APIError> {
        tracing::info!("Saving mnemonic to database");
        Mnemonic::delete_many()
            .exec(&self.db)
            .await
            .map_err(|e| APIError::DatabaseError(e.to_string()))?;

        let new_mnemonic = mnemonic::ActiveModel {
            id: ActiveValue::NotSet,
            encrypted_mnemonic: ActiveValue::Set(encrypted_mnemonic),
        };

        new_mnemonic
            .insert(&self.db)
            .await
            .map_err(|e| APIError::DatabaseError(e.to_string()))?;
        tracing::info!("Mnemonic saved successfully");

        Ok(())
    }

    pub async fn load_mnemonic(&self) -> Result<String, APIError> {
        tracing::info!("Loading mnemonic from database");
        let mnemonic_model = Mnemonic::find()
            .one(&self.db)
            .await
            .map_err(|e| APIError::DatabaseError(e.to_string()))?
            .ok_or(APIError::NotInitialized)?;
        tracing::info!("Mnemonic loaded successfully");

        Ok(mnemonic_model.encrypted_mnemonic)
    }

    pub async fn check_already_initialized(&self) -> Result<bool, APIError> {
        tracing::info!("Checking if already initialized");
        let mnemonics = Mnemonic::find()
            .all(&self.db)
            .await
            .map_err(|e| APIError::DatabaseError(e.to_string()))?;
        let initialized = !mnemonics.is_empty();
        tracing::info!("Already initialized: {}", initialized);
        Ok(initialized)
    }

    pub async fn save_channel_peer(
        &self,
        pubkey: &PublicKey,
        address: &SocketAddr,
    ) -> Result<(), APIError> {
        tracing::info!("Saving channel peer to database: {}@{}", pubkey, address);

        // Delete existing entry for this pubkey if it exists
        ChannelPeerData::delete_many()
            .filter(channel_peer_data::Column::PublicKey.eq(pubkey.to_string()))
            .exec(&self.db)
            .await
            .map_err(|e| APIError::DatabaseError(e.to_string()))?;

        // Insert new entry
        let new_peer = channel_peer_data::ActiveModel {
            id: ActiveValue::NotSet,
            public_key: ActiveValue::Set(pubkey.to_string()),
            socket_addr: ActiveValue::Set(address.to_string()),
        };

        new_peer
            .insert(&self.db)
            .await
            .map_err(|e| APIError::DatabaseError(e.to_string()))?;

        tracing::info!("Channel peer saved successfully");
        Ok(())
    }

    pub async fn load_channel_peers(&self) -> Result<HashMap<PublicKey, SocketAddr>, APIError> {
        tracing::debug!("Loading channel peers from database");

        let peers = ChannelPeerData::find()
            .all(&self.db)
            .await
            .map_err(|e| APIError::DatabaseError(e.to_string()))?;

        let mut peer_data = HashMap::new();
        for peer in peers {
            let pubkey: PublicKey = peer
                .public_key
                .parse()
                .map_err(|e| APIError::InvalidPeerInfo(format!("Invalid public key: {}", e)))?;
            let socket_addr: SocketAddr = peer
                .socket_addr
                .parse()
                .map_err(|e| APIError::InvalidPeerInfo(format!("Invalid socket address: {}", e)))?;
            peer_data.insert(pubkey, socket_addr);
        }

        tracing::debug!("Loaded {} channel peers from database", peer_data.len());
        Ok(peer_data)
    }

    pub async fn delete_channel_peer(&self, pubkey: &PublicKey) -> Result<(), APIError> {
        tracing::info!("Deleting channel peer from database: {}", pubkey);

        let result: DeleteResult = ChannelPeerData::delete_many()
            .filter(channel_peer_data::Column::PublicKey.eq(pubkey.to_string()))
            .exec(&self.db)
            .await
            .map_err(|e| APIError::DatabaseError(e.to_string()))?;

        if result.rows_affected > 0 {
            tracing::info!("Channel peer deleted successfully");
        } else {
            tracing::warn!("Channel peer not found for deletion: {}", pubkey);
        }

        Ok(())
    }

    pub async fn save_rgb_config(&self, key: &str, value: &str) -> Result<(), APIError> {
        tracing::info!("Saving RGB config to database: {} = {}", key, value);

        let existing = RgbConfig::find()
            .filter(rgb_config::Column::Key.eq(key))
            .one(&self.db)
            .await
            .map_err(|e| APIError::DatabaseError(e.to_string()))?;

        if let Some(model) = existing {
            let mut active_model: rgb_config::ActiveModel = model.into();
            active_model.value = ActiveValue::Set(value.to_string());
            active_model
                .update(&self.db)
                .await
                .map_err(|e| APIError::DatabaseError(e.to_string()))?;
        } else {
            let new_config = rgb_config::ActiveModel {
                id: ActiveValue::NotSet,
                key: ActiveValue::Set(key.to_string()),
                value: ActiveValue::Set(value.to_string()),
            };
            new_config
                .insert(&self.db)
                .await
                .map_err(|e| APIError::DatabaseError(e.to_string()))?;
        }

        // Update cache with new value
        self.rgb_config_cache.lock().await.insert(key.to_string(), Some(value.to_string()));

        tracing::info!("RGB config saved successfully");
        Ok(())
    }

    pub async fn load_rgb_config(&self, key: &str) -> Result<Option<String>, APIError> {
        tracing::debug!("Loading RGB config from cache/database: {}", key);

        // Check cache first
        {
            let cache = self.rgb_config_cache.lock().await;
            if let Some(cached_value) = cache.get(key) {
                return Ok(cached_value.clone());
            }
        }

        // Load from database and cache it
        let config = RgbConfig::find()
            .filter(rgb_config::Column::Key.eq(key))
            .one(&self.db)
            .await
            .map_err(|e| APIError::DatabaseError(e.to_string()))?;

        let value = config.map(|c| c.value);
        {
            let mut cache = self.rgb_config_cache.lock().await;
            cache.insert(key.to_string(), value.clone());
        }

        Ok(value)
    }

    pub async fn migrate_indexer_url_from_file(&self, storage_dir: &Path) -> Result<(), APIError> {
        const INDEXER_URL_FNAME: &str = "indexer_url";

        let indexer_url_path = storage_dir.join(INDEXER_URL_FNAME);

        if !indexer_url_path.exists() {
            tracing::info!("No existing indexer_url file found, skipping migration");
            return Ok(());
        }

        tracing::info!("Found existing indexer_url file, migrating to database");

        let indexer_url = fs::read_to_string(&indexer_url_path)
            .map_err(|e| APIError::IO(e))?
            .trim()
            .to_string();

        self.save_rgb_config("indexer_url", &indexer_url).await?;

        tracing::info!("Successfully migrated indexer_url from file to database");

        Ok(())
    }

    /// Syncs RGB config values from database to filesystem files.
    /// This is necessary because the rust-lightning library reads these values directly from files
    /// during RGB wallet operations (e.g., _get_indexer_url, _accept_transfer).
    /// The database is the source of truth, but files serve as a read-only cache for library compatibility.
    pub async fn sync_rgb_config_to_files(&self, storage_dir: &Path) -> Result<(), APIError> {
        const INDEXER_URL_FNAME: &str = "indexer_url";
        const PROXY_ENDPOINT_FNAME: &str = "proxy_endpoint";

        let indexer_url = self.load_rgb_config("indexer_url").await?;
        let proxy_endpoint = self.load_rgb_config("proxy_endpoint").await?;

        if let Some(url) = indexer_url {
            let indexer_url_path = storage_dir.join(INDEXER_URL_FNAME);
            fs::write(&indexer_url_path, url).map_err(|e| APIError::IO(e))?;
            tracing::info!("Synced indexer_url to file");
        }

        if let Some(proxy) = proxy_endpoint {
            let proxy_endpoint_path = storage_dir.join(PROXY_ENDPOINT_FNAME);
            fs::write(&proxy_endpoint_path, proxy).map_err(|e| APIError::IO(e))?;
            tracing::info!("Synced proxy_endpoint to file");
        }

        Ok(())
    }
}
