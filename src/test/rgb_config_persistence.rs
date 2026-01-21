use std::fs;
use tempfile::TempDir;
use crate::database::DatabaseManager;

const INDEXER_URL_FNAME: &str = "indexer_url";
const PROXY_ENDPOINT_FNAME: &str = "proxy_endpoint";

#[tokio::test]
async fn test_save_and_load_rgb_config() {
    // Create a temporary database
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_manager = DatabaseManager::new(&db_path).await.unwrap();

    // Test saving and loading indexer_url
    let indexer_url = "127.0.0.1:50001";
    db_manager.save_rgb_config("indexer_url", indexer_url).await.unwrap();
    let loaded = db_manager.load_rgb_config("indexer_url").await.unwrap();
    assert_eq!(loaded, Some(indexer_url.to_string()));

    // Test saving and loading proxy_endpoint
    let proxy_endpoint = "rpc://127.0.0.1:3000/json-rpc";
    db_manager.save_rgb_config("proxy_endpoint", proxy_endpoint).await.unwrap();
    let loaded = db_manager.load_rgb_config("proxy_endpoint").await.unwrap();
    assert_eq!(loaded, Some(proxy_endpoint.to_string()));

    // Test loading non-existent key
    let loaded = db_manager.load_rgb_config("non_existent").await.unwrap();
    assert_eq!(loaded, None);

    // Test updating existing key
    let new_indexer_url = "127.0.0.1:50002";
    db_manager.save_rgb_config("indexer_url", new_indexer_url).await.unwrap();
    let loaded = db_manager.load_rgb_config("indexer_url").await.unwrap();
    assert_eq!(loaded, Some(new_indexer_url.to_string()));
}

#[tokio::test]
async fn test_rgb_config_cache() {
    // Create a temporary database
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_manager = DatabaseManager::new(&db_path).await.unwrap();

    // Save a value
    let indexer_url = "127.0.0.1:50001";
    db_manager.save_rgb_config("indexer_url", indexer_url).await.unwrap();

    // First load should hit DB and cache
    let loaded1 = db_manager.load_rgb_config("indexer_url").await.unwrap();
    assert_eq!(loaded1, Some(indexer_url.to_string()));

    // Second load should hit cache
    let loaded2 = db_manager.load_rgb_config("indexer_url").await.unwrap();
    assert_eq!(loaded2, Some(indexer_url.to_string()));

    // Verify cache is updated on save
    let new_indexer_url = "127.0.0.1:50002";
    db_manager.save_rgb_config("indexer_url", new_indexer_url).await.unwrap();
    let loaded3 = db_manager.load_rgb_config("indexer_url").await.unwrap();
    assert_eq!(loaded3, Some(new_indexer_url.to_string()));
}

#[tokio::test]
async fn test_sync_rgb_config_to_files() {
    // Create a temporary database and directory
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_manager = DatabaseManager::new(&db_path).await.unwrap();

    // Save config values
    let indexer_url = "127.0.0.1:50001";
    let proxy_endpoint = "rpc://127.0.0.1:3000/json-rpc";
    db_manager.save_rgb_config("indexer_url", indexer_url).await.unwrap();
    db_manager.save_rgb_config("proxy_endpoint", proxy_endpoint).await.unwrap();

    // Sync to files
    db_manager.sync_rgb_config_to_files(temp_dir.path()).await.unwrap();

    // Verify files exist and contain correct content
    let indexer_file = temp_dir.path().join(INDEXER_URL_FNAME);
    let proxy_file = temp_dir.path().join(PROXY_ENDPOINT_FNAME);

    assert!(indexer_file.exists());
    assert!(proxy_file.exists());

    let indexer_content = fs::read_to_string(&indexer_file).unwrap();
    let proxy_content = fs::read_to_string(&proxy_file).unwrap();

    assert_eq!(indexer_content.trim(), indexer_url);
    assert_eq!(proxy_content.trim(), proxy_endpoint);
}

#[tokio::test]
async fn test_migrate_indexer_url_from_file() {
    // Create a temporary directory
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_manager = DatabaseManager::new(&db_path).await.unwrap();

    // Create a file with indexer_url
    let indexer_url = "127.0.0.1:50001";
    let indexer_file = temp_dir.path().join(INDEXER_URL_FNAME);
    fs::write(&indexer_file, indexer_url).unwrap();

    // Migrate from file to DB
    db_manager.migrate_indexer_url_from_file(temp_dir.path()).await.unwrap();

    // Verify value is in DB
    let loaded = db_manager.load_rgb_config("indexer_url").await.unwrap();
    assert_eq!(loaded, Some(indexer_url.to_string()));

    // Verify file still exists (migration doesn't delete it)
    assert!(indexer_file.exists());
    let file_content = fs::read_to_string(&indexer_file).unwrap();
    assert_eq!(file_content.trim(), indexer_url);
}

#[tokio::test]
async fn test_migrate_no_file_present() {
    // Create a temporary directory without indexer_url file
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_manager = DatabaseManager::new(&db_path).await.unwrap();

    // Try to migrate when no file exists
    db_manager.migrate_indexer_url_from_file(temp_dir.path()).await.unwrap();

    // Verify no value is set in DB
    let loaded = db_manager.load_rgb_config("indexer_url").await.unwrap();
    assert_eq!(loaded, None);
}

#[tokio::test]
async fn test_sync_empty_config_to_files() {
    // Create a temporary database and directory
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_manager = DatabaseManager::new(&db_path).await.unwrap();

    // No config saved, sync to files
    db_manager.sync_rgb_config_to_files(temp_dir.path()).await.unwrap();

    // Verify no files are created
    let indexer_file = temp_dir.path().join(INDEXER_URL_FNAME);
    let proxy_file = temp_dir.path().join(PROXY_ENDPOINT_FNAME);

    assert!(!indexer_file.exists());
    assert!(!proxy_file.exists());
}

#[tokio::test]
async fn test_sync_partial_config_to_files() {
    // Create a temporary database and directory
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_manager = DatabaseManager::new(&db_path).await.unwrap();

    // Save only indexer_url
    let indexer_url = "127.0.0.1:50001";
    db_manager.save_rgb_config("indexer_url", indexer_url).await.unwrap();

    // Sync to files
    db_manager.sync_rgb_config_to_files(temp_dir.path()).await.unwrap();

    // Verify only indexer_url file exists
    let indexer_file = temp_dir.path().join(INDEXER_URL_FNAME);
    let proxy_file = temp_dir.path().join(PROXY_ENDPOINT_FNAME);

    assert!(indexer_file.exists());
    assert!(!proxy_file.exists());

    let indexer_content = fs::read_to_string(&indexer_file).unwrap();
    assert_eq!(indexer_content.trim(), indexer_url);
}

#[tokio::test]
async fn test_overwrite_file_on_sync() {
    // Create a temporary database and directory
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_manager = DatabaseManager::new(&db_path).await.unwrap();

    // Create file with old content
    let indexer_file = temp_dir.path().join(INDEXER_URL_FNAME);
    fs::write(&indexer_file, "old.url:9999").unwrap();

    // Save new config and sync
    let new_indexer_url = "127.0.0.1:50001";
    db_manager.save_rgb_config("indexer_url", new_indexer_url).await.unwrap();
    db_manager.sync_rgb_config_to_files(temp_dir.path()).await.unwrap();

    // Verify file content is overwritten
    let indexer_content = fs::read_to_string(&indexer_file).unwrap();
    assert_eq!(indexer_content.trim(), new_indexer_url);
}
