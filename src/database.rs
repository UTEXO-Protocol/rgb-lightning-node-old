use crate::entities::{channel_peer_data, mnemonic, prelude::*};
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

pub struct DatabaseManager {
    db: DatabaseConnection,
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

        Ok(Self { db })
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
}
