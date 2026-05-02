//! Onboarding IPC: mnemonic lifecycle, vault, AXL boot, direct ENS registration on Sepolia.

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use alloy::ens::namehash;
use alloy::primitives::{keccak256, Address, Bytes, B256, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::signers::Signer;
use alloy::signers::local::PrivateKeySigner;
use anton_core::crypto::ed25519::Ed25519Identity;
use anton_core::crypto::kdf::KdfParams;
use anton_core::crypto::mnemonic::MnemonicPhrase;
use anton_core::crypto::vault::Vault;
use anton_core::crypto::wallet::Wallet;
use anton_core::settings::Settings;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};
use tauri::State;

use crate::session::{IdentitySessionState, UnlockedIdentity};
use crate::sidecar::{AxlSidecar, AxlSidecarState};

/// Ethereum Sepolia chain id — EIP-155 txs from [`register_username`].
const SEPOLIA_CHAIN_ID: u64 = 11155111;
const DEFAULT_ENS_REGISTRY_ADDRESS: &str = "0x00000000000C2E074eC69A0dFb2997BA6C7d2e1e";
const DEFAULT_SEPOLIA_PUBLIC_RESOLVER_ADDRESS: &str =
    "0xE99638b40E4Fff0129D56f03b55b6bbC4BBE49b5";
const DEFAULT_SEPOLIA_NAME_WRAPPER_ADDRESS: &str =
    "0x0635513f179D50A207757E05759CbD106d7dFcE8";
const DEFAULT_ENS_SEPOLIA_RPC_URL: &str = "https://ethereum-sepolia.publicnode.com";
const DEFAULT_ENS_PARENT_NAME: &str = "anton.eth";

alloy::sol! {
    #[sol(rpc)]
    contract EnsRegistry {
        function owner(bytes32 node) external view returns (address);
        function setSubnodeRecord(bytes32 node, bytes32 label, address owner, address resolver, uint64 ttl) external;
        function setOwner(bytes32 node, address owner) external;
    }

    #[sol(rpc)]
    contract PublicResolver {
        function setAddr(bytes32 node, address a) external;
        function setText(bytes32 node, string calldata key, string calldata value) external;
    }

    #[sol(rpc)]
    contract NameWrapper {
        function setSubnodeRecord(
            bytes32 parentNode,
            string calldata label,
            address owner,
            address resolver,
            uint64 ttl,
            uint32 fuses,
            uint64 expiry
        ) external returns (bytes32);
        function safeTransferFrom(address from, address to, uint256 id, uint256 amount, bytes calldata data) external;
    }
}

pub fn vault_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("vault.bin"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedPreviewResponse {
    pub ethereum_address: String,
    pub peer_id_hex: String,
    pub pubkey_pem: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitVaultResponse {
    pub ethereum_address: String,
    pub peer_id_hex: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlockVaultResponse {
    /// Full ENS name from settings after a successful `register_username`, if any.
    pub ens: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckUsernameResponse {
    pub available: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterUsernameResponse {
    pub tx_hash: String,
    pub ens: String,
}

fn settings_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(Settings::default_path(&dir))
}

fn normalize_label(raw: &str) -> Result<String, String> {
    let label = raw.trim().to_ascii_lowercase();
    if label.len() < 3 || label.len() > 63 {
        return Err("Username must be between 3 and 63 characters.".into());
    }
    if !label.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err("Username may only contain lowercase letters, digits, and hyphens.".into());
    }
    Ok(label)
}

fn ens_registration_rpc_url() -> String {
    std::env::var("ENS_RPC_URL")
        .or_else(|_| std::env::var("ENS_MAINNET_RPC_URL"))
        .unwrap_or_else(|_| DEFAULT_ENS_SEPOLIA_RPC_URL.into())
}

fn env_address(var: &str, default: &str) -> Result<Address, String> {
    let s = std::env::var(var).unwrap_or_else(|_| default.to_string());
    s.parse::<Address>()
        .map_err(|e| format!("{var} is not a valid address: {e}"))
}

fn ens_registry_address() -> Result<Address, String> {
    env_address("ENS_REGISTRY_ADDRESS", DEFAULT_ENS_REGISTRY_ADDRESS)
}

fn ens_public_resolver_address() -> Result<Address, String> {
    env_address(
        "ENS_PUBLIC_RESOLVER_ADDRESS",
        DEFAULT_SEPOLIA_PUBLIC_RESOLVER_ADDRESS,
    )
}

fn ens_name_wrapper_address() -> Result<Address, String> {
    env_address("ENS_NAME_WRAPPER_ADDRESS", DEFAULT_SEPOLIA_NAME_WRAPPER_ADDRESS)
}

fn ens_parent_name() -> String {
    std::env::var("ANTON_ENS_PARENT_NAME").unwrap_or_else(|_| DEFAULT_ENS_PARENT_NAME.into())
}

fn ens_name_for_label(label: &str) -> String {
    format!("{label}.{}", ens_parent_name())
}

fn ens_nodes_for_label(label: &str) -> (B256, B256, B256) {
    let parent_name = ens_parent_name();
    let full_name = format!("{label}.{parent_name}");
    (
        namehash(&parent_name),
        keccak256(label.as_bytes()),
        namehash(&full_name),
    )
}

fn ens_token_id(node: B256) -> U256 {
    U256::from_be_slice(node.as_slice())
}

/// Signs `register_username` txs on Sepolia L1. Gas is paid by this key (the `anton.eth`
/// operator wallet), not the user's derived wallet — final ENS owner remains the user.
fn registration_gas_signer() -> Result<PrivateKeySigner, String> {
    let raw = std::env::var("ANTON_ENS_REGISTRATION_PRIVATE_KEY")
        .or_else(|_| std::env::var("ANTON_REGISTRATION_GAS_PRIVATE_KEY"))
        .map_err(|_| {
            "Set ANTON_ENS_REGISTRATION_PRIVATE_KEY to the hex private key of the Sepolia wallet that owns/manages anton.eth."
                .to_string()
        })?;
    PrivateKeySigner::from_str(raw.trim())
        .map_err(|e| format!("ANTON_ENS_REGISTRATION_PRIVATE_KEY: {e}"))
        .map(|s| s.with_chain_id(Some(SEPOLIA_CHAIN_ID)))
}

fn derive_from_mnemonic(mnemonic: &MnemonicPhrase) -> Result<(Wallet, Ed25519Identity), String> {
    let seed = mnemonic.to_seed("");
    let wallet = Wallet::from_seed(&*seed).map_err(|e| e.to_string())?;
    let ed25519 = Ed25519Identity::from_seed(&*seed).map_err(|e| e.to_string())?;
    Ok((wallet, ed25519))
}

async fn boot_sidecar<R: Runtime>(
    app: &AppHandle<R>,
    sidecar_state: &AxlSidecarState,
    ed25519: &Ed25519Identity,
) -> Result<(), String> {
    sidecar_state.shutdown();
    let merged = crate::sidecar::merged_bootstrap_peers(app).await;
    let sidecar = AxlSidecar::launch(app, ed25519, Some(merged))
        .await
        .map_err(|e| e.to_string())?;
    sidecar_state.install(Arc::new(sidecar));
    Ok(())
}

#[tauri::command]
pub fn onboarding_generate_mnemonic() -> Result<String, String> {
    let phrase = MnemonicPhrase::generate_12().map_err(|e| e.to_string())?;
    Ok(phrase.as_str().to_owned())
}

#[tauri::command]
pub fn onboarding_derived_preview(mnemonic: String) -> Result<DerivedPreviewResponse, String> {
    let mnemonic = MnemonicPhrase::parse(&mnemonic).map_err(|e| e.to_string())?;
    let (wallet, ed25519) = derive_from_mnemonic(&mnemonic)?;
    Ok(DerivedPreviewResponse {
        ethereum_address: wallet.address().to_checksum(None),
        peer_id_hex: ed25519.peer_id_hex(),
        pubkey_pem: ed25519.to_public_pkcs8_pem().map_err(|e| e.to_string())?,
    })
}

#[tauri::command]
pub async fn onboarding_commit_vault<R: Runtime>(
    app: AppHandle<R>,
    sidecar_state: State<'_, AxlSidecarState>,
    session_state: State<'_, IdentitySessionState>,
    mnemonic: String,
    passphrase: String,
) -> Result<CommitVaultResponse, String> {
    let mnemonic = MnemonicPhrase::parse(&mnemonic).map_err(|e| e.to_string())?;
    let vault_path = vault_path(&app)?;
    let vault = Vault::new(&mnemonic, None);
    vault
        .save(&vault_path, &passphrase, KdfParams::default())
        .map_err(|e| e.to_string())?;

    let (wallet, ed25519) = derive_from_mnemonic(&mnemonic)?;
    let ethereum_address = wallet.address().to_checksum(None);
    let peer_id_hex = ed25519.peer_id_hex();

    boot_sidecar(&app, &sidecar_state, &ed25519).await?;

    session_state.set(UnlockedIdentity { wallet, ed25519 });

    Ok(CommitVaultResponse {
        ethereum_address,
        peer_id_hex,
    })
}

#[tauri::command]
pub fn vault_exists<R: Runtime>(app: AppHandle<R>) -> Result<bool, String> {
    let path = vault_path(&app)?;
    Ok(path.exists())
}

#[tauri::command]
pub async fn unlock_vault<R: Runtime>(
    app: AppHandle<R>,
    sidecar_state: State<'_, AxlSidecarState>,
    session_state: State<'_, IdentitySessionState>,
    passphrase: String,
) -> Result<UnlockVaultResponse, String> {
    let vault_path = vault_path(&app)?;
    let vault = Vault::load(&vault_path, &passphrase).map_err(|e| e.to_string())?;
    let mnemonic = vault.mnemonic().map_err(|e| e.to_string())?;
    let (wallet, ed25519) = derive_from_mnemonic(&mnemonic)?;

    boot_sidecar(&app, &sidecar_state, &ed25519).await?;

    session_state.set(UnlockedIdentity { wallet, ed25519 });

    let settings_path = settings_path(&app)?;
    let settings = Settings::load_or_default(&settings_path).map_err(|e| e.to_string())?;

    Ok(UnlockVaultResponse {
        ens: settings.last_username.clone(),
    })
}

#[tauri::command]
pub async fn onboarding_check_username(label: String) -> Result<CheckUsernameResponse, String> {
    let label = normalize_label(&label)?;
    let registry = ens_registry_address()?;
    let url = ens_registration_rpc_url()
        .parse()
        .map_err(|_| "Invalid ENS_RPC_URL.".to_string())?;
    let provider = ProviderBuilder::new().connect_http(url);
    let registry = EnsRegistry::new(registry, provider);
    let (_, _, node) = ens_nodes_for_label(&label);
    let owner = registry
        .owner(node)
        .call()
        .await
        .map_err(|e| e.to_string())?;

    let available = owner == Address::ZERO;
    Ok(CheckUsernameResponse { available })
}

#[tauri::command]
pub async fn register_username<R: Runtime>(
    app: AppHandle<R>,
    session_state: State<'_, IdentitySessionState>,
    label: String,
) -> Result<RegisterUsernameResponse, String> {
    let label = normalize_label(&label)?;
    let Some(id) = session_state.snapshot() else {
        return Err("Unlock your vault or finish onboarding before registering.".into());
    };

    let registry_addr = ens_registry_address()?;
    let resolver_addr = ens_public_resolver_address()?;
    let name_wrapper_addr = ens_name_wrapper_address()?;
    let rpc_url = ens_registration_rpc_url()
        .parse()
        .map_err(|_| "Invalid ENS_RPC_URL.".to_string())?;

    let gas_signer = registration_gas_signer()?;
    let operator = gas_signer.address();

    let provider = ProviderBuilder::new()
        .wallet(gas_signer)
        .connect_http(rpc_url);

    let pem = id.ed25519.to_public_pkcs8_pem().map_err(|e| e.to_string())?;
    let owner = id.wallet.address();
    let peer_id = id.ed25519.peer_id_hex();
    let (parent_node, labelhash, node) = ens_nodes_for_label(&label);

    let registry = EnsRegistry::new(registry_addr, &provider);
    let resolver = PublicResolver::new(resolver_addr, &provider);
    let name_wrapper = NameWrapper::new(name_wrapper_addr, &provider);

    let existing_owner = registry
        .owner(node)
        .call()
        .await
        .map_err(|e| format!("check ENS owner: {e}"))?;
    if existing_owner != Address::ZERO {
        return Err(format!("{}.{} is already registered.", label, ens_parent_name()));
    }

    let parent_owner = registry
        .owner(parent_node)
        .call()
        .await
        .map_err(|e| format!("check parent ENS owner: {e}"))?;

    let pending = if parent_owner == name_wrapper_addr {
        name_wrapper
            .setSubnodeRecord(parent_node, label.clone(), operator, resolver_addr, 0, 0, 0)
            .send()
            .await
            .map_err(|e| format!("create wrapped ENS subname: {e}"))?
    } else {
        registry
            .setSubnodeRecord(parent_node, labelhash, operator, resolver_addr, 0)
            .send()
            .await
            .map_err(|e| format!("create ENS subname: {e}"))?
    };
    let create_tx = pending
        .watch()
        .await
        .map_err(|e| format!("wait for create subname: {e}"))?;

    let pending = resolver
        .setAddr(node, owner)
        .send()
        .await
        .map_err(|e| format!("set addr(60): {e}"))?;
    pending
        .watch()
        .await
        .map_err(|e| format!("wait for set addr(60): {e}"))?;

    let pending = resolver
        .setText(node, "axl_peer_id".to_string(), peer_id)
        .send()
        .await
        .map_err(|e| format!("set axl_peer_id: {e}"))?;
    pending
        .watch()
        .await
        .map_err(|e| format!("wait for set axl_peer_id: {e}"))?;

    let pending = resolver
        .setText(node, "axl_pubkey".to_string(), pem)
        .send()
        .await
        .map_err(|e| format!("set axl_pubkey: {e}"))?;
    pending
        .watch()
        .await
        .map_err(|e| format!("wait for set axl_pubkey: {e}"))?;

    let pending = if parent_owner == name_wrapper_addr {
        name_wrapper
            .safeTransferFrom(operator, owner, ens_token_id(node), U256::from(1), Bytes::new())
            .send()
            .await
            .map_err(|e| format!("transfer wrapped ENS subname to user: {e}"))?
    } else {
        registry
            .setOwner(node, owner)
            .send()
            .await
            .map_err(|e| format!("transfer ENS subname to user: {e}"))?
    };

    let tx_hash = pending
        .watch()
        .await
        .map_err(|e| format!("wait for transfer owner: {e}"))?;

    let receipt = provider
        .get_transaction_receipt(tx_hash)
        .await
        .map_err(|e| format!("get receipt: {e}"))?
        .ok_or_else(|| "transaction receipt not found".to_string())?;

    if !receipt.status() {
        return Err("Registration transaction reverted on-chain.".into());
    }

    tracing::info!(
        target = "anton::onboarding",
        create_tx = format!("{create_tx:#x}"),
        owner_tx = format!("{tx_hash:#x}"),
        ens = ens_name_for_label(&label),
        "registered direct ENS subname"
    );

    let ens = ens_name_for_label(&label);
    let settings_path = settings_path(&app)?;
    let mut settings = Settings::load_or_default(&settings_path).map_err(|e| e.to_string())?;
    settings.last_username = Some(ens.clone());
    settings.save(&settings_path).map_err(|e| e.to_string())?;

    Ok(RegisterUsernameResponse {
        tx_hash: format!("{:#x}", tx_hash),
        ens,
    })
}
