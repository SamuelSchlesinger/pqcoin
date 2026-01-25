//! JSON-RPC API for pqcoin.
//!
//! Provides a Bitcoin-compatible RPC interface for wallets and tools.

use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::server::{RpcModule, Server};
use jsonrpsee::types::ErrorObjectOwned;
use serde::{Deserialize, Serialize};

use super::ApiState;
use crate::blockchain::{
    Block, Deserialize as BlockDeserialize, Serialize as BlockSerialize, Transaction,
};
use crate::crypto::Hash;

/// Blockchain info response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainInfo {
    /// Current blockchain height.
    pub height: u64,
    /// Best block hash (hex).
    pub best_block_hash: String,
    /// Current difficulty bits.
    pub difficulty: u32,
    /// Number of transactions in mempool.
    pub mempool_size: usize,
}

/// Block info response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockInfo {
    /// Block hash (hex).
    pub hash: String,
    /// Block height.
    pub height: u64,
    /// Block version.
    pub version: u32,
    /// Previous block hash (hex).
    pub prev_hash: String,
    /// Merkle root (hex).
    pub merkle_root: String,
    /// Block timestamp.
    pub timestamp: u64,
    /// Difficulty bits.
    pub difficulty: u32,
    /// Nonce (hex-encoded 32 bytes).
    pub nonce: String,
    /// Number of transactions.
    pub tx_count: usize,
    /// Transaction IDs (hex).
    pub txids: Vec<String>,
}

/// Mempool info response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolInfo {
    /// Number of transactions in mempool.
    pub size: usize,
    /// Transaction IDs (hex).
    pub txids: Vec<String>,
}

/// Peer info response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    /// Peer ID.
    pub id: u64,
    /// Peer address.
    pub addr: String,
}

/// UTXO info response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtxoInfo {
    /// Transaction ID (hex).
    pub txid: String,
    /// Output index.
    pub vout: u32,
    /// Amount in quanta.
    pub amount: u64,
    /// Block height where the UTXO was created.
    pub height: u64,
    /// Whether this is a coinbase output.
    pub is_coinbase: bool,
}

/// Transaction info response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxInfo {
    /// Transaction ID (hex).
    pub txid: String,
    /// Number of inputs.
    pub vin_count: usize,
    /// Number of outputs.
    pub vout_count: usize,
    /// Is coinbase transaction.
    pub is_coinbase: bool,
    /// Serialized transaction (hex).
    pub hex: String,
}

/// JSON-RPC trait definition for pqcoin.
#[rpc(server)]
pub trait PqcoinRpc {
    /// Get blockchain info.
    #[method(name = "getblockchaininfo")]
    async fn get_blockchain_info(&self) -> RpcResult<BlockchainInfo>;

    /// Get block by hash.
    #[method(name = "getblock")]
    async fn get_block(&self, hash: String) -> RpcResult<BlockInfo>;

    /// Get block hash by height.
    #[method(name = "getblockhash")]
    async fn get_block_hash(&self, height: u64) -> RpcResult<String>;

    /// Get mempool info.
    #[method(name = "getmempoolinfo")]
    async fn get_mempool_info(&self) -> RpcResult<MempoolInfo>;

    /// Get connected peers.
    #[method(name = "getpeerinfo")]
    async fn get_peer_info(&self) -> RpcResult<Vec<PeerInfo>>;

    /// Send raw transaction (hex-encoded).
    #[method(name = "sendrawtransaction")]
    async fn send_raw_transaction(&self, tx_hex: String) -> RpcResult<String>;

    /// Get balance for an address (hex-encoded address hash).
    #[method(name = "getbalance")]
    async fn get_balance(&self, address: String) -> RpcResult<u64>;

    /// Get UTXOs for an address.
    #[method(name = "getutxos")]
    async fn get_utxos(&self, address: String) -> RpcResult<Vec<UtxoInfo>>;

    /// Get raw transaction by txid.
    #[method(name = "getrawtransaction")]
    async fn get_raw_transaction(&self, txid: String) -> RpcResult<TxInfo>;

    /// Get current difficulty.
    #[method(name = "getdifficulty")]
    async fn get_difficulty(&self) -> RpcResult<u32>;

    /// Get best block hash.
    #[method(name = "getbestblockhash")]
    async fn get_best_block_hash(&self) -> RpcResult<String>;

    /// Get block count (height).
    #[method(name = "getblockcount")]
    async fn get_block_count(&self) -> RpcResult<u64>;
}

/// RPC server implementation.
pub struct RpcServerImpl {
    state: ApiState,
}

impl RpcServerImpl {
    pub fn new(state: ApiState) -> Self {
        Self { state }
    }
}

fn rpc_error(code: i32, message: &str) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(code, message, None::<()>)
}

fn parse_hash(hex: &str) -> Result<Hash, ErrorObjectOwned> {
    let bytes = hex::decode(hex).map_err(|_| rpc_error(-1, "Invalid hex string"))?;
    if bytes.len() != 64 {
        return Err(rpc_error(-1, "Invalid hash length (expected 64 bytes)"));
    }
    let mut arr = [0u8; 64];
    arr.copy_from_slice(&bytes);
    Ok(Hash::from_bytes(arr))
}

fn block_to_info(block: &Block, height: u64) -> BlockInfo {
    let hash = block.hash();
    BlockInfo {
        hash: hash.to_hex(),
        height,
        version: block.header.version,
        prev_hash: block.header.prev_hash.to_hex(),
        merkle_root: block.header.merkle_root.to_hex(),
        timestamp: block.header.timestamp,
        difficulty: block.header.difficulty_bits,
        nonce: hex::encode(block.header.nonce),
        tx_count: block.transactions.len(),
        txids: block
            .transactions
            .iter()
            .map(|tx| tx.txid().to_hex())
            .collect(),
    }
}

#[jsonrpsee::core::async_trait]
impl PqcoinRpcServer for RpcServerImpl {
    async fn get_blockchain_info(&self) -> RpcResult<BlockchainInfo> {
        let blockchain = self.state.blockchain.read().await;
        let mempool = self.state.mempool.read().await;

        Ok(BlockchainInfo {
            height: blockchain.height(),
            best_block_hash: blockchain.tip_hash().to_hex(),
            difficulty: blockchain.tip().header.difficulty_bits,
            mempool_size: mempool.len(),
        })
    }

    async fn get_block(&self, hash: String) -> RpcResult<BlockInfo> {
        let hash = parse_hash(&hash)?;
        let blockchain = self.state.blockchain.read().await;

        let block = blockchain
            .get_block(&hash)
            .ok_or_else(|| rpc_error(-5, "Block not found"))?;

        let height = blockchain
            .get_height(&hash)
            .ok_or_else(|| rpc_error(-5, "Block height not found"))?;

        Ok(block_to_info(block, height))
    }

    async fn get_block_hash(&self, height: u64) -> RpcResult<String> {
        let blockchain = self.state.blockchain.read().await;

        let hash = blockchain
            .hash_at_height(height)
            .ok_or_else(|| rpc_error(-8, "Block height out of range"))?;

        Ok(hash.to_hex())
    }

    async fn get_mempool_info(&self) -> RpcResult<MempoolInfo> {
        let mempool = self.state.mempool.read().await;

        Ok(MempoolInfo {
            size: mempool.len(),
            txids: mempool.txids().iter().map(|h| h.to_hex()).collect(),
        })
    }

    async fn get_peer_info(&self) -> RpcResult<Vec<PeerInfo>> {
        // Return empty for now - peer info requires network service access
        Ok(vec![])
    }

    async fn send_raw_transaction(&self, tx_hex: String) -> RpcResult<String> {
        let tx_bytes = hex::decode(&tx_hex).map_err(|_| rpc_error(-22, "Invalid hex"))?;

        let (tx, _) = Transaction::deserialize(&tx_bytes)
            .map_err(|e| rpc_error(-22, &format!("TX decode failed: {}", e)))?;

        let txid = tx.txid();

        // Add to mempool
        let blockchain = self.state.blockchain.read().await;
        let current_height = blockchain.height();
        let mut mempool = self.state.mempool.write().await;

        mempool
            .add(tx, &blockchain, current_height)
            .map_err(|e| rpc_error(-25, &format!("TX rejected: {}", e)))?;

        Ok(txid.to_hex())
    }

    async fn get_balance(&self, address: String) -> RpcResult<u64> {
        let hash = parse_hash(&address)?;
        let address = crate::blockchain::Address::from_hash(hash);

        let blockchain = self.state.blockchain.read().await;
        Ok(blockchain.balance(&address))
    }

    async fn get_utxos(&self, address: String) -> RpcResult<Vec<UtxoInfo>> {
        let hash = parse_hash(&address)?;
        let address = crate::blockchain::Address::from_hash(hash);

        let blockchain = self.state.blockchain.read().await;
        let utxos = blockchain.utxos_for_address(&address);

        Ok(utxos
            .into_iter()
            .map(|(outpoint, utxo)| UtxoInfo {
                txid: outpoint.txid.to_hex(),
                vout: outpoint.index,
                amount: utxo.output.amount,
                height: utxo.height,
                is_coinbase: utxo.is_coinbase,
            })
            .collect())
    }

    async fn get_raw_transaction(&self, txid: String) -> RpcResult<TxInfo> {
        let hash = parse_hash(&txid)?;

        // Search in blockchain
        let blockchain = self.state.blockchain.read().await;

        // Search all blocks for the transaction
        for block in blockchain.all_blocks() {
            for tx in &block.transactions {
                if tx.txid() == hash {
                    return Ok(TxInfo {
                        txid: hash.to_hex(),
                        vin_count: tx.inputs.len(),
                        vout_count: tx.outputs.len(),
                        is_coinbase: tx.is_coinbase(),
                        hex: hex::encode(tx.to_bytes()),
                    });
                }
            }
        }

        // Search in mempool
        let mempool = self.state.mempool.read().await;
        if let Some(tx) = mempool.get(&hash) {
            return Ok(TxInfo {
                txid: hash.to_hex(),
                vin_count: tx.inputs.len(),
                vout_count: tx.outputs.len(),
                is_coinbase: tx.is_coinbase(),
                hex: hex::encode(tx.to_bytes()),
            });
        }

        Err(rpc_error(-5, "Transaction not found"))
    }

    async fn get_difficulty(&self) -> RpcResult<u32> {
        let blockchain = self.state.blockchain.read().await;
        Ok(blockchain.tip().header.difficulty_bits)
    }

    async fn get_best_block_hash(&self) -> RpcResult<String> {
        let blockchain = self.state.blockchain.read().await;
        Ok(blockchain.tip_hash().to_hex())
    }

    async fn get_block_count(&self) -> RpcResult<u64> {
        let blockchain = self.state.blockchain.read().await;
        Ok(blockchain.height())
    }
}

/// Run the JSON-RPC server.
pub async fn run_rpc_server(
    state: ApiState,
    addr: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = Server::builder().build(addr).await?;

    let rpc_impl = RpcServerImpl::new(state);
    let mut module = RpcModule::new(());

    // Merge the RPC methods
    module.merge(rpc_impl.into_rpc())?;

    let handle = server.start(module);

    tracing::info!(addr = addr, "JSON-RPC server listening");

    handle.stopped().await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blockchain_info_serialization() {
        let info = BlockchainInfo {
            height: 100,
            best_block_hash: "abc123".to_string(),
            difficulty: 0x1d00ffff,
            mempool_size: 50,
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"height\":100"));
    }

    #[test]
    fn test_parse_hash_valid() {
        let hex = "0".repeat(128); // 64 bytes = 128 hex chars
        let result = parse_hash(&hex);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_hash_invalid_length() {
        let hex = "0".repeat(64); // 32 bytes = wrong length
        let result = parse_hash(&hex);
        assert!(result.is_err());
    }
}
