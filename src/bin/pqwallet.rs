//! pqwallet - CLI wallet for pqcoin.
//!
//! A command-line wallet for managing pqcoin addresses and transactions.

use clap::{Parser, Subcommand};
use pqcoin::blockchain::Address;
use pqcoin::crypto::PublicKey;
use pqcoin::wallet::{
    Mnemonic, PartialTransaction, TransactionBuilder, UtxoInput, Wallet, WalletError, WalletStorage,
};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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

        /// Create an HD wallet with mnemonic backup
        #[arg(long)]
        hd: bool,
    },

    /// Recover an HD wallet from mnemonic phrase
    Recover {
        /// Wallet name
        #[arg(short, long, default_value = "recovered")]
        name: String,
    },

    /// Show wallet address or derive a new one
    #[command(subcommand)]
    Address(AddressCommands),

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

        /// Create unsigned transaction (for watch-only wallets)
        #[arg(long)]
        unsigned: bool,

        /// Output file for unsigned transaction
        #[arg(long)]
        output: Option<PathBuf>,
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

    // === Watch-only wallet commands (Issue 009) ===
    /// Create a watch-only wallet
    Watch {
        /// Wallet name
        #[arg(short, long)]
        name: String,

        /// Public key (hex) to watch
        #[arg(long, conflicts_with = "address")]
        pubkey: Option<String>,

        /// Address (hex) to watch (limited - cannot create transactions)
        #[arg(long, conflicts_with = "pubkey")]
        address: Option<String>,
    },

    /// Sign a transaction with this wallet
    Sign {
        /// Input transaction file (unsigned or partial)
        #[arg(long)]
        tx: PathBuf,

        /// Output file for signed transaction
        #[arg(long)]
        output: PathBuf,
    },

    /// Broadcast a signed transaction
    Broadcast {
        /// Transaction file to broadcast
        #[arg(long)]
        tx: PathBuf,
    },

    // === Multisig commands (Issue 007) ===
    /// Multisig wallet operations
    #[command(subcommand)]
    Multisig(MultisigCommands),

    /// Export public key to a file (for multisig setup)
    Pubkey {
        /// Output file for the public key
        #[arg(long)]
        file: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum AddressCommands {
    /// Show the current wallet address
    Show,
    /// Derive a new address (HD wallets only)
    New,
}

#[derive(Subcommand, Debug)]
enum MultisigCommands {
    /// Create a new multisig address
    Create {
        /// Number of required signatures
        #[arg(long)]
        required: u8,

        /// Comma-separated list of public key files
        #[arg(long)]
        pubkeys: String,
    },

    /// Sign a multisig transaction
    Sign {
        /// Input transaction file
        #[arg(long)]
        tx: PathBuf,

        /// Output file for partially signed transaction
        #[arg(long)]
        output: PathBuf,
    },

    /// Combine partial signatures into a complete transaction
    Combine {
        /// Comma-separated list of partial transaction files
        #[arg(long)]
        partials: String,

        /// Output file for the combined transaction
        #[arg(long)]
        output: PathBuf,
    },
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

async fn cmd_create(cli: &Cli, name: &str, hd: bool) -> Result<(), WalletError> {
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

    if hd {
        // Create HD wallet with mnemonic
        let (wallet, mnemonic) = Wallet::create_hd(name, &password, path.clone())?;

        println!("HD Wallet created successfully!");
        println!("Address: {}", wallet.address().to_hex());
        println!("Saved to: {}", path.display());
        println!();
        println!("=== BACKUP YOUR SEED PHRASE ===");
        println!();
        for (i, word) in mnemonic.words().iter().enumerate() {
            println!("{:2}. {}", i + 1, word);
        }
        println!();
        println!("IMPORTANT: Write down these 24 words and store them securely.");
        println!("They are the ONLY way to recover your wallet if you lose access.");
        println!("Never share them with anyone!");
        println!();
        println!("Use 'pqwallet address new' to derive additional addresses.");
    } else {
        let wallet = Wallet::create(name, &password, path.clone())?;

        println!("Wallet created successfully!");
        println!("Address: {}", wallet.address().to_hex());
        println!("Saved to: {}", path.display());
        println!();
        println!("IMPORTANT: Remember your password. It cannot be recovered.");
    }

    Ok(())
}

async fn cmd_recover(cli: &Cli, name: &str) -> Result<(), WalletError> {
    let path = get_wallet_path(cli);

    if path.exists() {
        return Err(WalletError::InvalidFormat(format!(
            "Wallet already exists at {}",
            path.display()
        )));
    }

    println!("Enter your 24-word recovery phrase (space-separated):");
    let mut phrase = String::new();
    io::stdin()
        .read_line(&mut phrase)
        .map_err(|e| WalletError::Io(e.to_string()))?;

    let mnemonic = Mnemonic::from_phrase(phrase.trim())
        .map_err(|e| WalletError::InvalidFormat(format!("Invalid mnemonic: {e}")))?;

    // Optional: Ask for BIP-39 passphrase (different from wallet password)
    print!("Enter BIP-39 passphrase (leave empty for none): ");
    io::stdout()
        .flush()
        .map_err(|e| WalletError::Io(e.to_string()))?;
    let mut passphrase = String::new();
    io::stdin()
        .read_line(&mut passphrase)
        .map_err(|e| WalletError::Io(e.to_string()))?;
    let passphrase = passphrase.trim();

    let password =
        read_password("Enter new wallet password: ").map_err(|e| WalletError::Io(e.to_string()))?;

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

    // Recover the HD wallet from the mnemonic
    let wallet = Wallet::recover_hd(name, &mnemonic, passphrase, &password, path.clone())?;

    println!("HD Wallet recovered successfully!");
    println!("Address: {}", wallet.address().to_hex());
    println!("Saved to: {}", path.display());
    println!();
    println!("Use 'pqwallet address new' to derive additional addresses.");

    Ok(())
}

async fn cmd_address_show(cli: &Cli) -> Result<(), WalletError> {
    let path = get_wallet_path(cli);
    let wallet = Wallet::load(path)?;

    if wallet.is_hd() {
        println!("HD Wallet: {}", wallet.name);
        println!("Current address: {}", wallet.address().to_hex());
        println!();

        let addresses = wallet.derived_addresses()?;
        if addresses.len() > 1 {
            println!("All derived addresses ({}):", addresses.len());
            for (i, addr) in addresses.iter().enumerate() {
                let marker = if addr == &wallet.address().to_hex() {
                    " (current)"
                } else {
                    ""
                };
                println!("  {i}: {addr}{marker}");
            }
        }

        if let Ok(next_index) = wallet.next_derivation_index() {
            println!();
            println!("Next derivation index: {next_index}");
            println!("Use 'pqwallet address new' to derive a new address.");
        }
    } else {
        println!("{}", wallet.address().to_hex());
    }

    Ok(())
}

async fn cmd_address_new(cli: &Cli) -> Result<(), WalletError> {
    let path = get_wallet_path(cli);
    let mut wallet = Wallet::load(path)?;

    if !wallet.is_hd() {
        println!("This is not an HD wallet. It uses a single address.");
        println!();
        println!("To create an HD wallet with multiple addresses, use:");
        println!("  pqwallet create --hd --name my-hd-wallet");
        return Ok(());
    }

    // Need password to derive new address
    let password =
        read_password("Enter wallet password: ").map_err(|e| WalletError::Io(e.to_string()))?;

    let (new_address, index) = wallet.derive_new_address(&password)?;

    println!("Derived new address at index {index}:");
    println!("{}", new_address.to_hex());
    println!();
    println!("This is now your active address.");

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

async fn cmd_send(
    cli: &Cli,
    to: &str,
    amount: u64,
    fee: u64,
    unsigned: bool,
    output: Option<PathBuf>,
) -> Result<(), WalletError> {
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

    // For unsigned transactions, we need the public key
    let public_key = wallet.public_key().ok_or_else(|| {
        WalletError::InvalidFormat(
            "cannot create transaction: public key not available (address-only wallet)".into(),
        )
    })?;

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

    if unsigned {
        // Create unsigned transaction using PartialTransaction
        let output_path = output.ok_or_else(|| {
            WalletError::InvalidFormat("--output required with --unsigned".into())
        })?;

        let total_needed = amount + fee;
        let mut selected_utxos = Vec::new();
        let mut total_selected: u64 = 0;

        for utxo in &utxo_inputs {
            // Check coinbase maturity
            if utxo.is_coinbase && info.height < utxo.height + pqcoin::constants::COINBASE_MATURITY
            {
                continue;
            }
            if total_selected >= total_needed {
                break;
            }
            selected_utxos.push(utxo);
            total_selected += utxo.amount;
        }

        if total_selected < total_needed {
            return Err(WalletError::Crypto(format!(
                "insufficient funds: have {total_selected}, need {total_needed} (including {fee} fee)"
            )));
        }

        use pqcoin::wallet::psbt::{
            LockingConditionData, OutPointData, PartialInput, PartialOutput,
        };

        let mut partial = PartialTransaction::new();

        for utxo in &selected_utxos {
            partial.add_input(PartialInput {
                outpoint: OutPointData::from_outpoint(&utxo.outpoint),
                amount: utxo.amount,
                locking_condition: LockingConditionData::p2pkh(wallet.address(), Some(public_key)),
                signatures: vec![None],
            });
        }

        partial.add_output(PartialOutput::p2pkh(amount, &to_address));

        // Add change output if needed
        let change = total_selected - total_needed;
        if change > 0 {
            partial.add_output(PartialOutput::p2pkh(change, wallet.address()));
        }

        partial.set_fee(fee);
        partial.save(&output_path)?;

        println!("Unsigned transaction saved to: {}", output_path.display());
        println!(
            "Sign with: pqwallet sign --tx {} --output signed.tx",
            output_path.display()
        );
    } else {
        // Signed transaction (existing logic)
        if wallet.is_watch_only() {
            return Err(WalletError::WatchOnly);
        }

        // Unlock wallet
        let password =
            read_password("Enter wallet password: ").map_err(|e| WalletError::Io(e.to_string()))?;

        wallet.unlock(&password)?;

        let keypair = wallet.keypair().ok_or(WalletError::Locked)?;

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
    }

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
    if let Some(pk) = storage.public_key {
        println!("Public Key: {pk}");
    } else {
        println!("Public Key: (not available - address-only wallet)");
    }

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

// === Watch-only wallet commands (Issue 009) ===

async fn cmd_watch(
    _cli: &Cli,
    name: &str,
    pubkey: Option<&str>,
    address: Option<&str>,
) -> Result<(), WalletError> {
    let storage = WalletStorage::new();
    let path = storage.wallet_path(name);

    if path.exists() {
        return Err(WalletError::InvalidFormat(format!(
            "Wallet '{name}' already exists"
        )));
    }

    if let Some(pk_hex) = pubkey {
        // Watch-only with public key
        let pk_bytes = hex::decode(pk_hex)
            .map_err(|e| WalletError::InvalidFormat(format!("invalid public key hex: {e}")))?;

        let public_key = PublicKey::from_bytes(&pk_bytes)
            .ok_or_else(|| WalletError::InvalidFormat("invalid public key".into()))?;

        let wallet = Wallet::create_watch_only(name, &public_key, path.clone())?;

        println!("Watch-only wallet created!");
        println!("Name: {name}");
        println!("Address: {}", wallet.address().to_hex());
        println!("Saved to: {}", path.display());
        println!();
        println!("This wallet can monitor balance and create unsigned transactions.");
        println!("Sign transactions offline with the full wallet.");
    } else if let Some(addr_hex) = address {
        // Address-only watch wallet
        let addr_bytes = hex::decode(addr_hex)
            .map_err(|e| WalletError::InvalidFormat(format!("invalid address hex: {e}")))?;

        if addr_bytes.len() != 64 {
            return Err(WalletError::InvalidFormat(
                "address must be 64 bytes (128 hex chars)".into(),
            ));
        }

        let mut addr_arr = [0u8; 64];
        addr_arr.copy_from_slice(&addr_bytes);
        let addr = Address::from_hash(pqcoin::crypto::Hash::from_bytes(addr_arr));

        let wallet = Wallet::create_watch_only_address(name, &addr, path.clone())?;

        println!("Address-only watch wallet created!");
        println!("Name: {name}");
        println!("Address: {}", wallet.address().to_hex());
        println!("Saved to: {}", path.display());
        println!();
        println!("This wallet can only check balance.");
        println!("(Cannot create transactions without the public key)");
    } else {
        return Err(WalletError::InvalidFormat(
            "must specify either --pubkey or --address".into(),
        ));
    }

    Ok(())
}

async fn cmd_sign(cli: &Cli, tx_path: &Path, output_path: &Path) -> Result<(), WalletError> {
    let wallet_path = get_wallet_path(cli);
    let mut wallet = Wallet::load(wallet_path)?;

    if wallet.is_watch_only() {
        return Err(WalletError::WatchOnly);
    }

    // Unlock wallet
    let password =
        read_password("Enter wallet password: ").map_err(|e| WalletError::Io(e.to_string()))?;
    wallet.unlock(&password)?;

    let keypair = wallet.keypair().ok_or(WalletError::Locked)?;

    // Load the partial transaction
    let mut partial = PartialTransaction::load(tx_path)?;

    // Sign all inputs we can sign
    let mut signed_count = 0;
    for i in 0..partial.inputs.len() {
        match partial.sign_input(i, keypair.public_key(), keypair.secret_key()) {
            Ok(_) => signed_count += 1,
            Err(_) => continue, // Skip inputs we can't sign
        }
    }

    if signed_count == 0 {
        return Err(WalletError::InvalidFormat(
            "no inputs could be signed with this wallet".into(),
        ));
    }

    partial.save(output_path)?;

    println!("Signed {signed_count} input(s)");
    println!("Saved to: {}", output_path.display());

    if partial.is_complete() {
        println!();
        println!("Transaction is complete!");
        println!(
            "Broadcast with: pqwallet broadcast --tx {}",
            output_path.display()
        );
    } else {
        println!();
        println!("Transaction needs more signatures.");
    }

    Ok(())
}

async fn cmd_broadcast(cli: &Cli, tx_path: &Path) -> Result<(), WalletError> {
    // Load the partial transaction
    let partial = PartialTransaction::load(tx_path)?;

    if !partial.is_complete() {
        return Err(WalletError::InvalidFormat(
            "transaction is not fully signed".into(),
        ));
    }

    // Convert to a regular transaction
    let tx = partial.to_transaction()?;

    // Serialize and send
    let tx_hex = hex::encode(pqcoin::blockchain::Serialize::to_bytes(&tx));

    let txid: String =
        rpc_call(&cli.rpc, "sendrawtransaction", serde_json::json!([tx_hex])).await?;

    println!("Transaction broadcast!");
    println!("TXID: {txid}");

    Ok(())
}

// === Multisig commands (Issue 007) ===

async fn cmd_pubkey(cli: &Cli, file: &Path) -> Result<(), WalletError> {
    let wallet_path = get_wallet_path(cli);
    let wallet = Wallet::load(wallet_path)?;

    let public_key = wallet
        .public_key()
        .ok_or_else(|| WalletError::InvalidFormat("wallet does not have a public key".into()))?;

    let pk_hex = hex::encode(public_key.to_bytes());
    std::fs::write(file, &pk_hex).map_err(|e| WalletError::Io(e.to_string()))?;

    println!("Public key exported to: {}", file.display());
    println!("Key: {}", &pk_hex[..64]); // Show first 32 bytes

    Ok(())
}

async fn cmd_multisig_create(
    _cli: &Cli,
    required: u8,
    pubkeys_arg: &str,
) -> Result<(), WalletError> {
    // Parse the public key files
    let pubkey_files: Vec<&str> = pubkeys_arg.split(',').collect();

    if pubkey_files.len() < 2 {
        return Err(WalletError::InvalidFormat(
            "multisig requires at least 2 public keys".into(),
        ));
    }

    if required < 1 || required as usize > pubkey_files.len() {
        return Err(WalletError::InvalidFormat(format!(
            "required must be between 1 and {} (number of keys)",
            pubkey_files.len()
        )));
    }

    let mut public_keys = Vec::with_capacity(pubkey_files.len());

    for file_path in &pubkey_files {
        let pk_hex = std::fs::read_to_string(file_path.trim())
            .map_err(|e| WalletError::Io(format!("failed to read {file_path}: {e}")))?;

        let pk_bytes = hex::decode(pk_hex.trim())
            .map_err(|e| WalletError::InvalidFormat(format!("invalid hex in {file_path}: {e}")))?;

        let pk = PublicKey::from_bytes(&pk_bytes).ok_or_else(|| {
            WalletError::InvalidFormat(format!("invalid public key in {file_path}"))
        })?;

        public_keys.push(pk);
    }

    // Create the multisig locking condition
    let condition = pqcoin::blockchain::LockingCondition::multisig(required, public_keys.clone());

    // Compute an identifier by hashing the serialized condition
    let condition_bytes = pqcoin::blockchain::Serialize::to_bytes(&condition);
    let condition_hash = pqcoin::crypto::hash(&condition_bytes);
    let address = Address::from_hash(condition_hash);

    println!("Created {}-of-{} multisig:", required, public_keys.len());
    println!("Multisig ID: {}", address.to_hex());
    println!();
    println!("Public keys:");
    for (i, pk) in public_keys.iter().enumerate() {
        let pk_hex = hex::encode(pk.to_bytes());
        println!("  {}: {}...", i + 1, &pk_hex[..32]);
    }
    println!();
    println!("To spend from this multisig, create a transaction with inputs");
    println!("that have this multisig locking condition, then sign with {required} of the keys.");

    Ok(())
}

async fn cmd_multisig_sign(
    cli: &Cli,
    tx_path: &Path,
    output_path: &Path,
) -> Result<(), WalletError> {
    // This is the same as cmd_sign - multisig signing is handled by the same logic
    cmd_sign(cli, tx_path, output_path).await
}

async fn cmd_multisig_combine(
    _cli: &Cli,
    partials_arg: &str,
    output_path: &Path,
) -> Result<(), WalletError> {
    let partial_files: Vec<&str> = partials_arg.split(',').collect();

    if partial_files.len() < 2 {
        return Err(WalletError::InvalidFormat(
            "need at least 2 partial transactions to combine".into(),
        ));
    }

    // Load the first partial as the base
    let mut combined = PartialTransaction::load(PathBuf::from(partial_files[0].trim()).as_path())?;

    // Merge the rest
    for file_path in &partial_files[1..] {
        let other = PartialTransaction::load(PathBuf::from(file_path.trim()).as_path())?;
        combined.merge(&other)?;
    }

    combined.save(output_path)?;

    println!("Combined {} partial transactions", partial_files.len());
    println!("Saved to: {}", output_path.display());

    if combined.is_complete() {
        println!();
        println!("Transaction is complete!");
        println!(
            "Broadcast with: pqwallet broadcast --tx {}",
            output_path.display()
        );
    } else {
        // Show progress for each input
        println!();
        println!("Transaction needs more signatures:");
        for i in 0..combined.inputs.len() {
            if let Ok((signed, required)) = combined.signature_count(i) {
                println!("  Input {i}: {signed}/{required} signatures");
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Commands::Create { name, hd } => cmd_create(&cli, name, *hd).await,
        Commands::Recover { name } => cmd_recover(&cli, name).await,
        Commands::Address(subcmd) => match subcmd {
            AddressCommands::Show => cmd_address_show(&cli).await,
            AddressCommands::New => cmd_address_new(&cli).await,
        },
        Commands::Balance => cmd_balance(&cli).await,
        Commands::Send {
            to,
            amount,
            fee,
            unsigned,
            output,
        } => cmd_send(&cli, to, *amount, *fee, *unsigned, output.clone()).await,
        Commands::History { count } => cmd_history(&cli, *count).await,
        Commands::Export => cmd_export(&cli).await,
        Commands::List => cmd_list(&cli).await,
        Commands::Watch {
            name,
            pubkey,
            address,
        } => cmd_watch(&cli, name, pubkey.as_deref(), address.as_deref()).await,
        Commands::Sign { tx, output } => cmd_sign(&cli, tx, output).await,
        Commands::Broadcast { tx } => cmd_broadcast(&cli, tx).await,
        Commands::Pubkey { file } => cmd_pubkey(&cli, file).await,
        Commands::Multisig(subcmd) => match subcmd {
            MultisigCommands::Create { required, pubkeys } => {
                cmd_multisig_create(&cli, *required, pubkeys).await
            }
            MultisigCommands::Sign { tx, output } => cmd_multisig_sign(&cli, tx, output).await,
            MultisigCommands::Combine { partials, output } => {
                cmd_multisig_combine(&cli, partials, output).await
            }
        },
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
