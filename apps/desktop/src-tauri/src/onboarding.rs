//! Onboarding IPC: mnemonic lifecycle, vault, AXL boot, ChatRegistrar on Base Sepolia.

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use alloy::primitives::{Address, Bytes};
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

/// Base Sepolia chain id — EIP-155 txs from [`register_username`].
const BASE_SEPOLIA_CHAIN_ID: u64 = 84532;

alloy::sol! {
    #[sol(rpc)]
    contract ChatRegistrar {
        function available(string label) external view returns (bool);
        function registerWithRecords(string label, address owner_, bytes peerId, string pubkeyPem) external;
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

fn base_sepolia_rpc_url() -> String {
    std::env::var("ANTON_BASE_SEPOLIA_RPC_URL").unwrap_or_else(|_| "https://sepolia.base.org".into())
}

fn registrar_address() -> Result<Address, String> {
    let s = std::env::var("ANTON_CHAT_REGISTRAR").map_err(|_| {
        "Set ANTON_CHAT_REGISTRAR to your deployed ChatRegistrar contract address on Base Sepolia."
            .to_string()
    })?;
    s.parse::<Address>().map_err(|e| format!("ANTON_CHAT_REGISTRAR is not a valid address: {e}"))
}

/// Signs `register_username` txs on Base Sepolia. Gas is paid by this key (deployment / `anton.eth`
/// operator wallet), not the user's derived wallet — `owner_` on-chain remains the user's address.
fn registration_gas_signer() -> Result<PrivateKeySigner, String> {
    let raw = std::env::var("ANTON_REGISTRATION_GAS_PRIVATE_KEY").map_err(|_| {
        "Set ANTON_REGISTRATION_GAS_PRIVATE_KEY to the hex private key of the wallet that pays Base Sepolia gas for register_username (e.g. the deployment wallet that owns anton.eth)."
            .to_string()
    })?;
    PrivateKeySigner::from_str(raw.trim())
        .map_err(|e| format!("ANTON_REGISTRATION_GAS_PRIVATE_KEY: {e}"))
        .map(|s| s.with_chain_id(Some(BASE_SEPOLIA_CHAIN_ID)))
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
    let registrar = registrar_address()?;
    let url = base_sepolia_rpc_url()
        .parse()
        .map_err(|_| "Invalid ANTON_BASE_SEPOLIA_RPC_URL.".to_string())?;
    let provider = ProviderBuilder::new().connect_http(url);
    let contract = ChatRegistrar::new(registrar, provider);
    let available = contract
        .available(label)
        .call()
        .await
        .map_err(|e| e.to_string())?;

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

    let registrar = registrar_address()?;
    let rpc_url = base_sepolia_rpc_url()
        .parse()
        .map_err(|_| "Invalid ANTON_BASE_SEPOLIA_RPC_URL.".to_string())?;

    let gas_signer = registration_gas_signer()?;

    let provider = ProviderBuilder::new()
        .wallet(gas_signer)
        .connect_http(rpc_url);

    let pem = id.ed25519.to_public_pkcs8_pem().map_err(|e| e.to_string())?;
    let peer_bytes = id.ed25519.peer_id();

    let contract = ChatRegistrar::new(registrar, &provider);
    let owner = id.wallet.address();

    let pending = contract
        .registerWithRecords(
            label.clone(),
            owner,
            Bytes::copy_from_slice(&peer_bytes),
            pem,
        )
        .send()
        .await
        .map_err(|e| format!("send transaction: {e}"))?;

    let tx_hash = pending
        .watch()
        .await
        .map_err(|e| format!("wait for receipt: {e}"))?;

    let receipt = provider
        .get_transaction_receipt(tx_hash)
        .await
        .map_err(|e| format!("get receipt: {e}"))?
        .ok_or_else(|| "transaction receipt not found".to_string())?;

    if !receipt.status() {
        return Err("Registration transaction reverted on-chain.".into());
    }

    let ens = format!("{label}.anton.eth");
    let settings_path = settings_path(&app)?;
    let mut settings = Settings::load_or_default(&settings_path).map_err(|e| e.to_string())?;
    settings.last_username = Some(ens.clone());
    settings.save(&settings_path).map_err(|e| e.to_string())?;

    Ok(RegisterUsernameResponse {
        tx_hash: format!("{:#x}", tx_hash),
        ens,
    })
}
