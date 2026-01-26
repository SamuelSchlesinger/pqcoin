//! pqwallet - CLI wallet for pqcoin.
//!
//! A command-line wallet for managing pqcoin addresses and transactions.

use clap::{Parser, Subcommand};
use pqcoin::blockchain::Address;
use pqcoin::wallet::{TransactionBuilder, UtxoInput, Wallet, WalletError, WalletStorage};
use std::io::{self, Write};
use std::path::PathBuf;

/// pqwallet - CLI wallet for pqcoin
#[derive(Parser, Debug)]
#[command(name = "pqwallet")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to wallet file (default: ~/.pqcoin/wallet.json)
    #[arg(short, long, value_name = "FILE")]
    wallet: Option<PathBuf>,

    /// RPC server address
    #[arg(long, default_value = "http://127.0.0.1:8332")]
    rpc: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Create a new wallet
    Create {
        /// Wallet name
        #[arg(short, long, default_value = "main")]
        name: String,
    },

    /// Show wallet address
    Address,

    /// Show wallet balance
    Balance,

    /// Send coins to an address
    Send {
        /// Recipient address (hex)
        #[arg(long)]
        to: String,

        /// Amount to send (in quanta)
        #[arg(long)]
        amount: u64,

        /// Transaction fee (in quanta)
        #[arg(long, default_value = "1000")]
        fee: u64,
    },

    /// List recent transactions
    History {
        /// Number of transactions to show
        #[arg(short, long, default_value = "10")]
        count: usize,
    },

    /// Export wallet public key
    Export,

    /// List all wallets
    List,
}

fn read_password(prompt: &str) -> io::Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;

    let mut password = String::new();
    io::stdin().read_line(&mut password)?;
    Ok(password.trim().to_string())
}

fn get_wallet_path(cli: &Cli) -> PathBuf {
    cli.wallet.clone().unwrap_or_else(Wallet::default_path)
}

async fn rpc_call<T: serde::de::DeserializeOwned>(
    rpc_url: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<T, WalletError> {
    let client = reqwest::Client::new();

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1
    });

    let response = client
        .post(rpc_url)
        .json(&request)
        .send()
        .await
        .map_err(|e| WalletError::Rpc(format!("request failed: {e}")))?;

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| WalletError::Rpc(format!("parse failed: {e}")))?;

    if let Some(error) = body.get("error") {
        if !error.is_null() {
            return Err(WalletError::Rpc(format!(
                "RPC error: {}",
                error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown")
            )));
        }
    }

    let result = body
        .get("result")
        .ok_or_else(|| WalletError::Rpc("no result in response".into()))?;

    serde_json::from_value(result.clone())
        .map_err(|e| WalletError::Rpc(format!("result parse failed: {e}")))
}

#[derive(serde::Deserialize)]
struct UtxoResponse {
    txid: String,
    vout: u32,
    amount: u64,
    height: u64,
    is_coinbase: bool,
}

#[derive(serde::Deserialize)]
struct BlockchainInfoResponse {
    height: u64,
}

async fn cmd_create(cli: &Cli, name: &str) -> Result<(), WalletError> {
    let path = get_wallet_path(cli);

    if path.exists() {
        eprintln!("Wallet already exists at {}", path.display());
        eprintln!("Use --wallet to specify a different path");
        return Ok(());
    }

    let password =
        read_password("Enter wallet password: ").map_err(|e| WalletError::Io(e.to_string()))?;

    if password.len() < 8 {
        return Err(WalletError::InvalidFormat(
            "Password must be at least 8 characters".into(),
        ));
    }

    let confirm =
        read_password("Confirm password: ").map_err(|e| WalletError::Io(e.to_string()))?;

    if password != confirm {
        return Err(WalletError::InvalidFormat("Passwords do not match".into()));
    }

    let wallet = Wallet::create(name, &password, path.clone())?;

    println!("Wallet created successfully!");
    println!("Address: {}", wallet.address().to_hex());
    println!("Saved to: {}", path.display());
    println!();
    println!("IMPORTANT: Remember your password. It cannot be recovered.");

    Ok(())
}

async fn cmd_address(cli: &Cli) -> Result<(), WalletError> {
    let path = get_wallet_path(cli);
    let wallet = Wallet::load(path)?;

    println!("{}", wallet.address().to_hex());

    Ok(())
}

async fn cmd_balance(cli: &Cli) -> Result<(), WalletError> {
    let path = get_wallet_path(cli);
    let wallet = Wallet::load(path)?;

    let address_hex = wallet.address().to_hex();

    match rpc_call::<u64>(&cli.rpc, "getbalance", serde_json::json!([address_hex])).await {
        Ok(balance) => {
            let coins = balance as f64 / 1_000_000.0;
            println!("Balance: {coins} PQC ({balance} quanta)");
        }
        Err(e) => {
            eprintln!("RPC error: {e}");
            eprintln!("Make sure the pqcoin node is running with --rpc enabled");
        }
    }

    Ok(())
}

async fn cmd_send(cli: &Cli, to: &str, amount: u64, fee: u64) -> Result<(), WalletError> {
    let path = get_wallet_path(cli);
    let mut wallet = Wallet::load(path)?;

    // Parse recipient address
    let to_bytes = hex::decode(to)
        .map_err(|_| WalletError::InvalidFormat("invalid recipient address hex".into()))?;

    if to_bytes.len() != 64 {
        return Err(WalletError::InvalidFormat(
            "recipient address must be 64 bytes (128 hex chars)".into(),
        ));
    }

    let mut to_arr = [0u8; 64];
    to_arr.copy_from_slice(&to_bytes);
    let to_address = Address::from_hash(pqcoin::crypto::Hash::from_bytes(to_arr));

    // Unlock wallet
    let password =
        read_password("Enter wallet password: ").map_err(|e| WalletError::Io(e.to_string()))?;

    wallet.unlock(&password)?;

    let keypair = wallet.keypair().ok_or(WalletError::Locked)?;

    // Get UTXOs from RPC
    let address_hex = wallet.address().to_hex();
    let utxos: Vec<UtxoResponse> =
        rpc_call(&cli.rpc, "getutxos", serde_json::json!([address_hex])).await?;

    if utxos.is_empty() {
        return Err(WalletError::Crypto("no UTXOs available".into()));
    }

    // Get current height
    let info: BlockchainInfoResponse =
        rpc_call(&cli.rpc, "getblockchaininfo", serde_json::json!([])).await?;

    // Convert UTXOs with proper error handling
    let mut utxo_inputs: Vec<UtxoInput> = Vec::with_capacity(utxos.len());
    for u in utxos {
        let txid_bytes = hex::decode(&u.txid).map_err(|e| {
            WalletError::InvalidFormat(format!("invalid txid hex '{}': {}", u.txid, e))
        })?;

        if txid_bytes.len() != 64 {
            return Err(WalletError::InvalidFormat(format!(
                "txid must be 64 bytes, got {}",
                txid_bytes.len()
            )));
        }

        let mut txid_arr = [0u8; 64];
        txid_arr.copy_from_slice(&txid_bytes);

        utxo_inputs.push(UtxoInput {
            outpoint: pqcoin::blockchain::OutPoint::new(
                pqcoin::crypto::Hash::from_bytes(txid_arr),
                u.vout,
            ),
            amount: u.amount,
            height: u.height,
            is_coinbase: u.is_coinbase,
        });
    }

    // Build transaction
    let tx = TransactionBuilder::new()
        .with_utxos(utxo_inputs)
        .add_output(to_address, amount)
        .with_change_address(*wallet.address())
        .with_fee(fee)
        .with_current_height(info.height)
        .build(keypair)?;

    // Serialize and send
    let tx_hex = hex::encode(pqcoin::blockchain::Serialize::to_bytes(&tx));

    let txid: String =
        rpc_call(&cli.rpc, "sendrawtransaction", serde_json::json!([tx_hex])).await?;

    println!("Transaction sent!");
    println!("TXID: {txid}");

    Ok(())
}

async fn cmd_history(cli: &Cli, _count: usize) -> Result<(), WalletError> {
    let path = get_wallet_path(cli);
    let wallet = Wallet::load(path)?;

    println!("Transaction history for: {}", wallet.address());
    println!();
    println!("(Transaction history requires blockchain indexing, not yet implemented)");

    Ok(())
}

async fn cmd_export(cli: &Cli) -> Result<(), WalletError> {
    let path = get_wallet_path(cli);
    let storage = pqcoin::wallet::WalletFile::load(&path)?;
    let wallet = Wallet::load(path)?;

    println!("Wallet: {}", wallet.name);
    println!("Address: {}", wallet.address().to_hex());
    println!("Public Key: {}", storage.public_key);

    Ok(())
}

async fn cmd_list(_cli: &Cli) -> Result<(), WalletError> {
    let storage = WalletStorage::new();
    let wallets = storage.list_wallets()?;

    if wallets.is_empty() {
        println!("No wallets found.");
        println!("Create one with: pqwallet create");
    } else {
        println!("Available wallets:");
        for name in wallets {
            println!("  - {name}");
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Commands::Create { name } => cmd_create(&cli, name).await,
        Commands::Address => cmd_address(&cli).await,
        Commands::Balance => cmd_balance(&cli).await,
        Commands::Send { to, amount, fee } => cmd_send(&cli, to, *amount, *fee).await,
        Commands::History { count } => cmd_history(&cli, *count).await,
        Commands::Export => cmd_export(&cli).await,
        Commands::List => cmd_list(&cli).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
