use clap::{value_parser, Parser};
use rgb_lib::BitcoinNetwork;
use std::path::PathBuf;

use crate::auth::check_auth_args;
use crate::error::AppError;
use crate::utils::check_port_is_available;

#[derive(clap::ValueEnum, Clone, Debug)]
pub(crate) enum DatabaseType {
    Sqlite,
    Mysql,
    Postgresql,
}

impl std::fmt::Display for DatabaseType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatabaseType::Sqlite => write!(f, "sqlite"),
            DatabaseType::Mysql => write!(f, "mysql"),
            DatabaseType::Postgresql => write!(f, "postgresql"),
        }
    }
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path for the node storage directory
    storage_directory_path: PathBuf,

    /// Listening port of the daemon
    #[arg(long, default_value_t = 3001)]
    daemon_listening_port: u16,

    /// Listening port for LN peers
    #[arg(long, default_value_t = 9735)]
    ldk_peer_listening_port: u16,

    /// Bitcoin network
    #[arg(long, default_value_t = BitcoinNetwork::Testnet, value_parser = value_parser!(BitcoinNetwork))]
    network: BitcoinNetwork,

    /// Max allowed media size for upload (in MB)
    #[arg(long, default_value_t = 5)]
    max_media_upload_size_mb: u16,

    /// Root public key for biscuit token authentication (hex-encoded)
    #[arg(long)]
    root_public_key: Option<String>,

    /// Disable authentication
    #[arg(long, default_value_t = false)]
    disable_authentication: bool,

    /// Database type: sqlite, mysql, postgresql
    #[arg(long, default_value_t = DatabaseType::Sqlite)]
    database_type: DatabaseType,

    /// Database URL (required for mysql/postgresql)
    #[arg(long)]
    database_url: Option<String>,
}

pub(crate) struct UserArgs {
    pub(crate) storage_dir_path: PathBuf,
    pub(crate) daemon_listening_port: u16,
    pub(crate) ldk_peer_listening_port: u16,
    pub(crate) network: BitcoinNetwork,
    pub(crate) max_media_upload_size_mb: u16,
    pub(crate) root_public_key: Option<biscuit_auth::PublicKey>,
    pub(crate) database_type: DatabaseType,
    pub(crate) database_url: Option<String>,
}

pub(crate) fn parse_startup_args() -> Result<UserArgs, AppError> {
    let args = Args::parse();

    let network = args.network;

    let daemon_listening_port = args.daemon_listening_port;
    check_port_is_available(daemon_listening_port)?;
    let ldk_peer_listening_port = args.ldk_peer_listening_port;
    check_port_is_available(ldk_peer_listening_port)?;

    let root_public_key = check_auth_args(args.disable_authentication, args.root_public_key)?;

    Ok(UserArgs {
        storage_dir_path: args.storage_directory_path,
        daemon_listening_port,
        ldk_peer_listening_port,
        network,
        max_media_upload_size_mb: args.max_media_upload_size_mb,
        root_public_key,
        database_type: args.database_type,
        database_url: args.database_url,
    })
}
