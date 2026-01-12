use super::*;
use crate::args::DatabaseType;
use tempfile::TempDir;
use tracing_test::traced_test;

#[traced_test]
#[tokio::test]
async fn test_database_connection_sqlite() {
    let temp_dir = TempDir::new().unwrap();
    let args = UserArgs {
        storage_dir_path: temp_dir.path().to_path_buf(),
        daemon_listening_port: 3001,
        ldk_peer_listening_port: 9735,
        network: rgb_lib::BitcoinNetwork::Testnet,
        max_media_upload_size_mb: 5,
        root_public_key: None,
        database_type: DatabaseType::Sqlite,
        database_url: None,
    };

    let app_state = crate::utils::start_daemon(&args).await.unwrap();

    // test ping
    app_state.static_state.db.ping().await.unwrap();

    // check db file created
    let db_path = temp_dir.path().join("db.sqlite");
    assert!(db_path.exists(), "SQLite database file should be created");
}

#[tokio::test]
async fn test_database_connection_invalid_mysql() {
    let temp_dir = TempDir::new().unwrap();
    let args = UserArgs {
        storage_dir_path: temp_dir.path().to_path_buf(),
        daemon_listening_port: 3001,
        ldk_peer_listening_port: 9735,
        network: rgb_lib::BitcoinNetwork::Testnet,
        max_media_upload_size_mb: 5,
        root_public_key: None,
        database_type: DatabaseType::Mysql,
        database_url: None, // missing url should cause error
    };

    let result = crate::utils::start_daemon(&args).await;
    assert!(
        result.is_err(),
        "Should fail without database URL for MySQL"
    );
    if let Err(err) = result {
        match err {
            crate::error::AppError::ConfigError(msg) => {
                assert!(
                    msg.contains("Database URL required"),
                    "Error should mention missing URL"
                );
            }
            _ => panic!("Expected ConfigError, got {:?}", err),
        }
    }
}

#[tokio::test]
async fn test_database_connection_invalid_postgresql() {
    let temp_dir = TempDir::new().unwrap();
    let args = UserArgs {
        storage_dir_path: temp_dir.path().to_path_buf(),
        daemon_listening_port: 3001,
        ldk_peer_listening_port: 9735,
        network: rgb_lib::BitcoinNetwork::Testnet,
        max_media_upload_size_mb: 5,
        root_public_key: None,
        database_type: DatabaseType::Postgresql,
        database_url: None, // missing url should cause error
    };

    let result = crate::utils::start_daemon(&args).await;
    assert!(
        result.is_err(),
        "Should fail without database URL for PostgreSQL"
    );
    if let Err(err) = result {
        match err {
            crate::error::AppError::ConfigError(msg) => {
                assert!(
                    msg.contains("Database URL required"),
                    "Error should mention missing URL"
                );
            }
            _ => panic!("Expected ConfigError, got {:?}", err),
        }
    }
}
