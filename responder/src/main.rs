use axum::{
    Router,
    extract::{Query, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Json},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use solana_hash::Hash;
use solana_instruction::{AccountMeta, Instruction};
use solana_message::Message;
use solana_pubkey::Pubkey;
use solana_transaction::Transaction;
use std::{
    fs,
    io::Write,
    net::SocketAddr,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, RwLock},
};
use tower_http::cors::{Any, CorsLayer};

// ── State ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventConfig {
    pub event_id:    String,
    pub title:       String,
    pub description: String,
    pub icon_url:    String,
    pub capacity:    u32,
    pub tiers:       Vec<PriceTier>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceTier {
    pub label:       String,
    pub amount_usdc: f64,
    pub active:      bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RosterState {
    pub confirmed:  u32,
    pub waitlisted: u32,
}

/// On-disk shape of the state file. The event fields stay at the top level so
/// files written by earlier versions (a bare `EventConfig`) still load; the
/// roster is an additive field that defaults to zero when absent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedState {
    #[serde(flatten)]
    pub event:  EventConfig,
    #[serde(default)]
    pub roster: RosterState,
}

#[derive(Debug)]
pub struct AppState {
    pub event:      RwLock<Option<EventConfig>>,
    pub roster:     RwLock<RosterState>,
    pub state_path: PathBuf,
    pub rpc_url:    String,
    pub recipient:  String,
    pub devnet:     bool,
}

impl AppState {
    pub fn load(state_path: PathBuf) -> Arc<Self> {
        let (event, roster) = if state_path.exists() {
            let raw = fs::read_to_string(&state_path).unwrap_or_default();
            match serde_json::from_str::<PersistedState>(&raw) {
                Ok(p)  => (Some(p.event), p.roster),
                // Legacy state files hold a bare EventConfig with no roster.
                Err(_) => (
                    serde_json::from_str::<EventConfig>(&raw).ok(),
                    RosterState::default(),
                ),
            }
        } else {
            (None, RosterState::default())
        };

        // TURNSTILE_RPC is the documented name; TURNSTILE_RPC_URL is kept as a
        // fallback so existing deployments keep working.
        let rpc_url = std::env::var("TURNSTILE_RPC")
            .or_else(|_| std::env::var("TURNSTILE_RPC_URL"))
            .unwrap_or_else(|_| "https://api.devnet.solana.com".into());

        let recipient = std::env::var("TURNSTILE_RECIPIENT")
            .unwrap_or_else(|_| "11111111111111111111111111111111".into());

        let devnet = std::env::var("TURNSTILE_DEVNET")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(true);

        Arc::new(Self {
            event:  RwLock::new(event),
            roster: RwLock::new(roster),
            state_path,
            rpc_url,
            recipient,
            devnet,
        })
    }

    pub fn spots_remaining(&self) -> Option<u32> {
        let event  = self.event.read().ok()?;
        let roster = self.roster.read().ok()?;
        let cap    = event.as_ref()?.capacity;
        Some(cap.saturating_sub(roster.confirmed))
    }

    pub fn active_tier(&self) -> Option<PriceTier> {
        let event = self.event.read().ok()?;
        event.as_ref()?
            .tiers
            .iter()
            .find(|t| t.active)
            .cloned()
    }
}

// ── Action types (Solana Actions spec) ───────────────────────────────────────

#[derive(Serialize)]
struct ActionsJson {
    rules: Vec<ActionRule>,
}

#[derive(Serialize)]
struct ActionRule {
    #[serde(rename = "pathPattern")]
    path_pattern: String,
    #[serde(rename = "apiPath")]
    api_path:     String,
}

#[derive(Serialize)]
struct ActionGetResponse {
    title:       String,
    icon:        String,
    description: String,
    label:       String,
    links:       ActionLinks,
}

#[derive(Serialize)]
struct ActionLinks {
    actions: Vec<LinkedAction>,
}

#[derive(Serialize)]
struct LinkedAction {
    label:  String,
    href:   String,
}

#[derive(Serialize)]
struct ActionPostResponse {
    transaction: String,
    message:     String,
}

#[derive(Deserialize)]
struct EnrollQuery {
    event_id: Option<String>,
}

#[derive(Deserialize)]
struct EnrollPostBody {
    account: String,
}

// ── Admin types ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AdminEventBody {
    event_id:    String,
    title:       String,
    description: String,
    icon_url:    String,
    capacity:    u32,
    tiers:       Vec<PriceTier>,
}

#[derive(Deserialize)]
struct AdminTierBody {
    event_id:   String,
    tier_label: String,
}

#[derive(Deserialize)]
struct AdminConfirmBody {
    event_id:  String,
    /// `ref` is a Rust keyword, so the wire name is mapped onto `reference`.
    #[serde(rename = "ref")]
    reference: String,
    signature: String,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn actions_json() -> impl IntoResponse {
    Json(ActionsJson {
        rules: vec![ActionRule {
            path_pattern: "/enroll*".into(),
            api_path:     "/actions/enroll*".into(),
        }],
    })
}

async fn enroll_get(
    State(state): State<Arc<AppState>>,
    Query(params): Query<EnrollQuery>,
) -> impl IntoResponse {
    let event = match state.event.read() {
        Ok(g)  => g.clone(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "state lock poisoned").into_response(),
    };

    let event = match event.as_ref() {
        Some(e) => e.clone(),
        None    => return (StatusCode::SERVICE_UNAVAILABLE, "no active event").into_response(),
    };

    if let Some(id) = &params.event_id {
        if id != &event.event_id {
            return (StatusCode::NOT_FOUND, "event not found").into_response();
        }
    }

    let spots = state.spots_remaining().unwrap_or(0);
    let tier  = state.active_tier();

    let (label, description) = match &tier {
        Some(t) if spots > 0 => (
            format!("Enroll — {} USDC ({} spots left)", t.amount_usdc, spots),
            format!("{}\n\n{} spot(s) remaining at {} pricing.", event.description, spots, t.label),
        ),
        Some(_) => (
            "Join waitlist".into(),
            format!("{}\n\nSold out — join the waitlist.", event.description),
        ),
        None => return (StatusCode::SERVICE_UNAVAILABLE, "no active price tier").into_response(),
    };

    Json(ActionGetResponse {
        title:       event.title.clone(),
        icon:        event.icon_url.clone(),
        description,
        label:       label.clone(),
        links: ActionLinks {
            actions: vec![LinkedAction {
                label,
                href: format!("/actions/enroll?event_id={}", event.event_id),
            }],
        },
    }).into_response()
}

async fn enroll_post(
    State(state): State<Arc<AppState>>,
    Query(params): Query<EnrollQuery>,
    Json(body): Json<EnrollPostBody>,
) -> impl IntoResponse {
    // Validate payer pubkey
    let payer_pk = match Pubkey::from_str(&body.account) {
        Ok(pk) => pk,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": "invalid account public key"
        }))).into_response(),
    };

    let event = match state.event.read() {
        Ok(g)  => g.clone(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": "state lock poisoned"
        }))).into_response(),
    };

    let event = match event.as_ref() {
        Some(e) => e.clone(),
        None    => return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({
            "error": "no active event"
        }))).into_response(),
    };

    if let Some(id) = &params.event_id {
        if id != &event.event_id {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": "event not found"
            }))).into_response();
        }
    }

    let tier = match state.active_tier() {
        Some(t) => t,
        None    => return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({
            "error": "no active price tier"
        }))).into_response(),
    };

    // Derive a proper reference key (SHA-256 of event_id:account, full 32 bytes)
    let reference_key = derive_reference_key(&event.event_id, &body.account);

    // Fetch a real recent blockhash from the RPC
    let blockhash = match fetch_latest_blockhash(&state.rpc_url).await {
        Ok(h)  => h,
        Err(e) => return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
            "error": format!("RPC error fetching blockhash: {e}")
        }))).into_response(),
    };

    // Build the transaction
    let recipient_pk = match Pubkey::from_str(&state.recipient) {
        Ok(pk) => pk,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": "invalid recipient pubkey in configuration"
        }))).into_response(),
    };

    let reference_pk = match Pubkey::from_str(&reference_key) {
        Ok(pk) => pk,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": "reference key derivation failed"
        }))).into_response(),
    };

    let tx_result = build_usdc_transfer(
        &payer_pk,
        &recipient_pk,
        &reference_pk,
        tier.amount_usdc,
        &format!("Turnstile:{}", event.event_id),
        state.devnet,
        blockhash,
    );

    let spots = state.spots_remaining().unwrap_or(0);
    let message = if spots > 0 {
        format!("Enroll in {} — {} USDC. Reference: {}", event.title, tier.amount_usdc, reference_key)
    } else {
        format!("Added to waitlist for {}. Reference: {}", event.title, reference_key)
    };

    match tx_result {
        Ok(tx_base64) => Json(ActionPostResponse {
            transaction: tx_base64,
            message,
        }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": format!("transaction build failed: {e}")
        }))).into_response(),
    }
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok", "service": "turnstile-actions" }))
}

/// POST /admin/event
async fn admin_event(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AdminEventBody>,
) -> impl IntoResponse {
    if !check_admin_auth(&headers) {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "unauthorized" }))).into_response();
    }

    let config = EventConfig {
        event_id:    body.event_id,
        title:       body.title,
        description: body.description,
        icon_url:    body.icon_url,
        capacity:    body.capacity,
        tiers:       body.tiers,
    };

    // Reset in-memory state first so the roster reset is what gets persisted;
    // persisting before the reset would write the previous event's count.
    match state.event.write() {
        Ok(mut g)  => *g = Some(config.clone()),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": "state lock poisoned"
        }))).into_response(),
    }
    match state.roster.write() {
        Ok(mut g)  => *g = RosterState::default(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": "roster lock poisoned"
        }))).into_response(),
    }

    match persist_state(&state, &config) {
        Ok(_)  => Json(serde_json::json!({ "ok": true, "event_id": config.event_id })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": format!("persist failed: {e}")
        }))).into_response(),
    }
}

/// POST /admin/tier
async fn admin_tier(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AdminTierBody>,
) -> impl IntoResponse {
    if !check_admin_auth(&headers) {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "unauthorized" }))).into_response();
    }

    let mut event_guard = state.event.write().unwrap();
    let event = match event_guard.as_mut() {
        Some(e) if e.event_id == body.event_id => e,
        _ => return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "event not found" }))).into_response(),
    };

    let mut found = false;
    for tier in &mut event.tiers {
        tier.active = tier.label.to_lowercase() == body.tier_label.to_lowercase();
        if tier.active { found = true; }
    }

    if !found {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "tier not found" }))).into_response();
    }

    let config = event.clone();
    drop(event_guard);

    match persist_state(&state, &config) {
        Ok(_)  => Json(serde_json::json!({ "ok": true, "active_tier": body.tier_label })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": format!("persist failed: {e}")
        }))).into_response(),
    }
}

/// POST /admin/confirm — called by the payment-poll SOP after verifying getSignaturesForAddress
async fn admin_confirm(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AdminConfirmBody>,
) -> impl IntoResponse {
    if !check_admin_auth(&headers) {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "unauthorized" }))).into_response();
    }

    // A confirmation only counts against the currently active event, so a stale
    // cron tick for a previous event cannot inflate this event's roster.
    let event = match state.event.read() {
        Ok(g)  => g.clone(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": "state lock poisoned"
        }))).into_response(),
    };

    let event = match event {
        Some(e) if e.event_id == body.event_id => e,
        Some(e) => return (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error":        "event_id does not match the active event",
            "requested":    body.event_id,
            "active_event": e.event_id,
        }))).into_response(),
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({
            "error": "no active event"
        }))).into_response(),
    };

    let confirmed = {
        let mut roster = match state.roster.write() {
            Ok(g)  => g,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": "roster lock poisoned"
            }))).into_response(),
        };
        roster.confirmed = roster.confirmed.saturating_add(1);
        roster.confirmed
    }; // write guard dropped before persist_state takes a read guard

    if let Err(e) = persist_state(&state, &event) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": format!("persist failed: {e}")
        }))).into_response();
    }

    println!(
        "[confirm] event={} ref={} sig={} total_confirmed={}",
        body.event_id, body.reference, body.signature, confirmed
    );

    Json(serde_json::json!({
        "ok":              true,
        "confirmed":       confirmed,
        "spots_remaining": event.capacity.saturating_sub(confirmed),
        "ref":             body.reference,
    })).into_response()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn check_admin_auth(headers: &HeaderMap) -> bool {
    let token = std::env::var("TURNSTILE_ADMIN_TOKEN").unwrap_or_default();
    if token.is_empty() { return false; }
    headers.get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == format!("Bearer {}", token))
        .unwrap_or(false)
}

fn persist_state(state: &AppState, config: &EventConfig) -> Result<(), String> {
    // Ensure parent directory exists (e.g. /data on Railway)
    if let Some(parent) = state.state_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    // Persist the roster alongside the event so a restart does not reset the
    // confirmed count and silently hand out already-sold spots again.
    let roster = state.roster.read().map_err(|_| "roster lock poisoned".to_string())?.clone();
    let persisted = PersistedState { event: config.clone(), roster };

    let json = serde_json::to_string_pretty(&persisted).map_err(|e| e.to_string())?;

    // Write to a temp file and rename so a crash mid-write cannot truncate the
    // existing state file.
    let tmp_path = state.state_path.with_extension("json.tmp");
    {
        let mut file = fs::File::create(&tmp_path).map_err(|e| e.to_string())?;
        file.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp_path, &state.state_path).map_err(|e| e.to_string())?;
    Ok(())
}

/// Derive a proper 32-byte reference key using SHA-256 of "event_id:account"
fn derive_reference_key(event_id: &str, account: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(format!("{}:{}", event_id, account).as_bytes());
    let bytes: [u8; 32] = h.finalize().into();
    bs58::encode(bytes).into_string()
}

/// Fetch the latest blockhash from the Solana RPC
async fn fetch_latest_blockhash(rpc_url: &str) -> Result<Hash, String> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getLatestBlockhash",
        "params": [{ "commitment": "confirmed" }]
    });

    let resp: serde_json::Value = client
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let blockhash_str = resp["result"]["value"]["blockhash"]
        .as_str()
        .ok_or_else(|| "missing blockhash in RPC response".to_string())?;

    // Decode base58 blockhash into Hash
    let bytes = bs58::decode(blockhash_str)
        .into_vec()
        .map_err(|e| e.to_string())?;

    let arr: [u8; 32] = bytes.try_into()
        .map_err(|_| "blockhash is not 32 bytes".to_string())?;

    Ok(Hash::from(arr))
}

// ── Transaction builder ───────────────────────────────────────────────────────

const USDC_MINT_DEVNET:  &str = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";
const USDC_MINT_MAINNET: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const SPL_TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const ATA_PROGRAM:       &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJe8bv";
const MEMO_PROGRAM:      &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
const SYSTEM_PROGRAM:    &str = "11111111111111111111111111111111";

/// Derive Associated Token Account using spl-associated-token-account
fn get_ata(wallet: &Pubkey, mint: &Pubkey) -> Pubkey {
    spl_associated_token_account::get_associated_token_address(wallet, mint)
}

fn build_usdc_transfer(
    payer:     &Pubkey,
    recipient: &Pubkey,
    reference: &Pubkey,
    amount:    f64,
    memo:      &str,
    devnet:    bool,
    blockhash: Hash,
) -> Result<String, String> {
    let mint_str = if devnet { USDC_MINT_DEVNET } else { USDC_MINT_MAINNET };
    let mint_pk  = Pubkey::from_str(mint_str).unwrap();
    let spl_pk   = Pubkey::from_str(SPL_TOKEN_PROGRAM).unwrap();
    let ata_pk   = Pubkey::from_str(ATA_PROGRAM).unwrap();
    let memo_pk  = Pubkey::from_str(MEMO_PROGRAM).unwrap();
    let sys_pk   = Pubkey::from_str(SYSTEM_PROGRAM).unwrap();

    let source_ata = get_ata(payer, &mint_pk);
    let dest_ata   = get_ata(recipient, &mint_pk);

    let amount_raw: u64 = (amount * 1_000_000.0) as u64;

    // Instruction 1: create payer ATA if it doesn't exist (idempotent)
    let create_source_ata_ix = Instruction {
        program_id: ata_pk,
        accounts: vec![
            AccountMeta { pubkey: *payer,    is_signer: true,  is_writable: true  },
            AccountMeta { pubkey: source_ata, is_signer: false, is_writable: true  },
            AccountMeta { pubkey: *payer,    is_signer: false, is_writable: false },
            AccountMeta { pubkey: mint_pk,   is_signer: false, is_writable: false },
            AccountMeta { pubkey: sys_pk,    is_signer: false, is_writable: false },
            AccountMeta { pubkey: spl_pk,    is_signer: false, is_writable: false },
        ],
        // Instruction discriminator 1 = CreateIdempotent
        data: vec![1u8],
    };

    // Instruction 2: create recipient ATA if it doesn't exist (idempotent)
    let create_dest_ata_ix = Instruction {
        program_id: ata_pk,
        accounts: vec![
            AccountMeta { pubkey: *payer,    is_signer: true,  is_writable: true  },
            AccountMeta { pubkey: dest_ata,  is_signer: false, is_writable: true  },
            AccountMeta { pubkey: *recipient, is_signer: false, is_writable: false },
            AccountMeta { pubkey: mint_pk,   is_signer: false, is_writable: false },
            AccountMeta { pubkey: sys_pk,    is_signer: false, is_writable: false },
            AccountMeta { pubkey: spl_pk,    is_signer: false, is_writable: false },
        ],
        data: vec![1u8],
    };

    // Instruction 3: SPL Token Transfer
    let mut transfer_data = vec![3u8];
    transfer_data.extend_from_slice(&amount_raw.to_le_bytes());

    let transfer_ix = Instruction {
        program_id: spl_pk,
        accounts: vec![
            AccountMeta { pubkey: source_ata, is_signer: false, is_writable: true  },
            AccountMeta { pubkey: dest_ata,   is_signer: false, is_writable: true  },
            AccountMeta { pubkey: *payer,     is_signer: true,  is_writable: false },
            AccountMeta { pubkey: *reference, is_signer: false, is_writable: false },
        ],
        data: transfer_data,
    };

    // Instruction 4: Memo (for reference tracking)
    let memo_ix = Instruction {
        program_id: memo_pk,
        accounts:   vec![],
        data:       memo.as_bytes().to_vec(),
    };

    let message = Message::new_with_blockhash(
        &[create_source_ata_ix, create_dest_ata_ix, transfer_ix, memo_ix],
        Some(payer),
        &blockhash,
    );

    let tx = Transaction {
        signatures: vec![[0u8; 64].into()],
        message,
    };

    let serialized = serialize_transaction(&tx)?;
    Ok(BASE64.encode(&serialized))
}

fn serialize_transaction(tx: &Transaction) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    out.push(tx.signatures.len() as u8);
    for _ in &tx.signatures {
        out.extend_from_slice(&[0u8; 64]);
    }
    out.extend_from_slice(&serialize_message(&tx.message)?);
    Ok(out)
}

fn serialize_message(msg: &solana_message::Message) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    out.push(msg.header.num_required_signatures);
    out.push(msg.header.num_readonly_signed_accounts);
    out.push(msg.header.num_readonly_unsigned_accounts);
    write_compact_u16(&mut out, msg.account_keys.len() as u16);
    for key in &msg.account_keys {
        out.extend_from_slice(key.as_ref());
    }
    out.extend_from_slice(msg.recent_blockhash.as_ref());
    write_compact_u16(&mut out, msg.instructions.len() as u16);
    for ix in &msg.instructions {
        out.push(ix.program_id_index);
        write_compact_u16(&mut out, ix.accounts.len() as u16);
        out.extend_from_slice(&ix.accounts);
        write_compact_u16(&mut out, ix.data.len() as u16);
        out.extend_from_slice(&ix.data);
    }
    Ok(out)
}

fn write_compact_u16(buf: &mut Vec<u8>, mut val: u16) {
    loop {
        let mut byte = (val & 0x7F) as u8;
        val >>= 7;
        if val != 0 { byte |= 0x80; }
        buf.push(byte);
        if val == 0 { break; }
    }
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state_path = PathBuf::from(
        std::env::var("TURNSTILE_STATE")
            .unwrap_or_else(|_| "/data/turnstile-state.json".into()),
    );

    let state = AppState::load(state_path);

    // CORS is applied to the public router only. Admin routes are never
    // browser-reachable cross-origin.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    let public = Router::new()
        .route("/.well-known/actions.json", get(actions_json))
        .route("/actions/enroll",           get(enroll_get).post(enroll_post))
        .route("/health",                   get(health))
        .layer(cors)
        .with_state(state.clone());

    // No CORS layer here — bearer auth is the access control.
    let admin = Router::new()
        .route("/admin/event",   post(admin_event))
        .route("/admin/tier",    post(admin_tier))
        .route("/admin/confirm", post(admin_confirm))
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    // TURNSTILE_ADMIN_BIND selects where the admin router is served:
    //   "shared" (default) — merged onto the public listener, reachable through
    //                        the deployment's single exposed port, bearer-only.
    //   "local"            — its own listener on 127.0.0.1 only. Remote callers
    //                        (e.g. a ZeroClaw cron on another host) cannot reach
    //                        it; use this when the responder runs beside the agent.
    let admin_bind = std::env::var("TURNSTILE_ADMIN_BIND")
        .unwrap_or_else(|_| "shared".into())
        .to_lowercase();

    let public_addr = SocketAddr::from(([0, 0, 0, 0], port));

    match admin_bind.as_str() {
        "local" => {
            let admin_port: u16 = std::env::var("TURNSTILE_ADMIN_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or_else(|| port.saturating_add(1));
            let admin_addr = SocketAddr::from(([127, 0, 0, 1], admin_port));

            println!("turnstile-actions (public) listening on http://{}", public_addr);
            println!("turnstile-actions (admin)  listening on http://{} — localhost only", admin_addr);

            let public_listener = tokio::net::TcpListener::bind(public_addr).await?;
            let admin_listener  = tokio::net::TcpListener::bind(admin_addr).await?;

            let public_srv = axum::serve(public_listener, public);
            let admin_srv  = axum::serve(admin_listener, admin);

            tokio::try_join!(public_srv.into_future(), admin_srv.into_future())?;
        }
        _ => {
            if admin_bind != "shared" {
                eprintln!(
                    "warning: unrecognised TURNSTILE_ADMIN_BIND={admin_bind:?}, falling back to \"shared\""
                );
            }

            println!("turnstile-actions listening on http://{}", public_addr);
            println!("admin routes served on the same listener (bearer auth, no CORS)");

            let app = public.merge(admin);
            let listener = tokio::net::TcpListener::bind(public_addr).await?;
            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}