use crate::entities::{mnemonic, prelude::*};
use crate::error::APIError;
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, ActiveValue, Database, DatabaseConnection, EntityTrait};
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
}
