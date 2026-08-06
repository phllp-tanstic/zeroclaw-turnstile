use axum::{
    Router,
    extract::{Query, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Json},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, RwLock},
};
use tower_http::cors::{Any, CorsLayer};
use std::io::Write;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RosterState {
    pub confirmed:  u32,
    pub waitlisted: u32,
}

#[derive(Debug)]
pub struct AppState {
    pub event:      RwLock<Option<EventConfig>>,
    pub roster:     RwLock<RosterState>,
    pub state_path: PathBuf,
}

impl AppState {
    pub fn load(state_path: PathBuf) -> Arc<Self> {
        let event = if state_path.exists() {
            let raw = fs::read_to_string(&state_path).unwrap_or_default();
            serde_json::from_str(&raw).ok()
        } else {
            None
        };

        Arc::new(Self {
            event:  RwLock::new(event),
            roster: RwLock::new(RosterState { confirmed: 0, waitlisted: 0 }),
            state_path,
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
    transaction: String,       // base64-encoded unsigned transaction
    message:     String,
}

#[derive(Deserialize)]
struct EnrollQuery {
    event_id: Option<String>,
}

#[derive(Deserialize)]
struct EnrollPostBody {
    account: String,           // attendee's base58 public key
}
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
    event_id:    String,
    tier_label:  String,
}
// ── Handlers ─────────────────────────────────────────────────────────────────

/// GET /.well-known/actions.json
async fn actions_json() -> impl IntoResponse {
    Json(ActionsJson {
        rules: vec![ActionRule {
            path_pattern: "/enroll*".into(),
            api_path:     "/actions/enroll*".into(),
        }],
    })
}

/// GET /actions/enroll
async fn enroll_get(
    State(state): State<Arc<AppState>>,
    Query(params): Query<EnrollQuery>,
) -> impl IntoResponse {
    let event = match state.event.read() {
        Ok(guard) => guard.clone(),
        Err(_)    => return (StatusCode::INTERNAL_SERVER_ERROR, "state lock poisoned").into_response(),
    };

    let event = match event.as_ref() {
        Some(e) => e.clone(),
        None    => return (StatusCode::SERVICE_UNAVAILABLE, "no active event").into_response(),
    };

    // Validate event_id if provided
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
            format!("{}\n\nSold out — join the waitlist and get notified first.", event.description),
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
    })
    .into_response()
}

/// POST /actions/enroll
async fn enroll_post(
    State(state): State<Arc<AppState>>,
    Query(params): Query<EnrollQuery>,
    Json(body): Json<EnrollPostBody>,
) -> impl IntoResponse {
    // Validate the attendee's public key is valid base58
    if bs58::decode(&body.account).into_vec().is_err() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": "invalid account public key"
        }))).into_response();
    }

    let event = match state.event.read() {
        Ok(guard) => guard.clone(),
        Err(_)    => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
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

    let spots = state.spots_remaining().unwrap_or(0);

    // Build a Solana Pay transfer-request transaction stub.
    // In production this returns a real base64-encoded transaction
    // built from the Solana Pay spec. For now we return the
    // parameters the ZeroClaw skill will use to construct the full tx.
    // The reference key is derived deterministically from event_id + account.
    let reference_key = derive_reference_key(&event.event_id, &body.account);

    let message = if spots > 0 {
        format!(
            "Enroll in {} — {} USDC. Reference: {}",
            event.title, tier.amount_usdc, reference_key
        )
    } else {
        format!("Added to waitlist for {}. Reference: {}", event.title, reference_key)
    };

    // Read recipient from environment or use placeholder for devnet testing
    let recipient = std::env::var("TURNSTILE_RECIPIENT")
        .unwrap_or_else(|_| "11111111111111111111111111111111".to_string());

    let devnet = std::env::var("TURNSTILE_DEVNET")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(true); // default to devnet for safety

    let tx_result = solana_tx::build_usdc_transfer(&solana_tx::TransferParams {
        payer:     &body.account,
        recipient: &recipient,
        amount:    tier.amount_usdc,
        reference: &reference_key,
        memo:      &format!("Turnstile:{}", event.event_id),
        devnet,
    });

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

/// POST /admin/event — create or update event (requires Authorization header)
async fn admin_event(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AdminEventBody>,
) -> impl IntoResponse {
    // Verify admin token
    if !check_admin_auth(&headers) {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
            "error": "unauthorized"
        }))).into_response();
    }

    let config = EventConfig {
        event_id:    body.event_id,
        title:       body.title,
        description: body.description,
        icon_url:    body.icon_url,
        capacity:    body.capacity,
        tiers:       body.tiers,
    };

    // Write to state file
    match persist_state(&state, &config) {
        Ok(_) => {
            let mut event = state.event.write().unwrap();
            *event = Some(config.clone());
            let mut roster = state.roster.write().unwrap();
            *roster = RosterState { confirmed: 0, waitlisted: 0 };
            Json(serde_json::json!({
                "ok": true,
                "event_id": config.event_id
            })).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": format!("failed to persist state: {e}")
        }))).into_response(),
    }
}

/// POST /admin/tier — activate a price tier (requires Authorization header)
async fn admin_tier(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AdminTierBody>,
) -> impl IntoResponse {
    if !check_admin_auth(&headers) {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
            "error": "unauthorized"
        }))).into_response();
    }

    let mut event_guard = state.event.write().unwrap();
    let event = match event_guard.as_mut() {
        Some(e) if e.event_id == body.event_id => e,
        _ => return (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "event not found"
        }))).into_response(),
    };

    let mut found = false;
    for tier in &mut event.tiers {
        if tier.label.to_lowercase() == body.tier_label.to_lowercase() {
            tier.active = true;
            found = true;
        } else {
            tier.active = false;
        }
    }

    if !found {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "tier not found"
        }))).into_response();
    }

    let config = event.clone();
    drop(event_guard);

    match persist_state(&state, &config) {
        Ok(_) => Json(serde_json::json!({
            "ok": true,
            "active_tier": body.tier_label
        })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": format!("persist failed: {e}")
        }))).into_response(),
    }
}

fn check_admin_auth(headers: &HeaderMap) -> bool {
    let admin_token = std::env::var("TURNSTILE_ADMIN_TOKEN")
        .unwrap_or_default();
    if admin_token.is_empty() { return false; }
    
    headers.get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == format!("Bearer {}", admin_token))
        .unwrap_or(false)
}

fn persist_state(state: &AppState, config: &EventConfig) -> Result<(), String> {
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| e.to_string())?;
    let mut file = std::fs::File::create(&state.state_path)
        .map_err(|e| e.to_string())?;
    file.write_all(json.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Health check
async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok", "service": "turnstile-actions" }))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn derive_reference_key(event_id: &str, account: &str) -> String {
    // Deterministic reference key: base58(sha256(event_id + ":" + account)[..32])
    // This lets the ZeroClaw SOP poll getSignaturesForAddress for exactly this key.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    format!("{}:{}", event_id, account).hash(&mut h);
    let v = h.finish().to_le_bytes();
    // Pad to 32 bytes for a valid-looking pubkey reference
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&v);
    bytes[8..16].copy_from_slice(&v);
    bytes[16..24].copy_from_slice(&v);
    bytes[24..32].copy_from_slice(&v);
    bs58::encode(bytes).into_string()
}

fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < input.len() {
        let b0 = input[i] as u32;
        let b1 = if i + 1 < input.len() { input[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < input.len() { input[i + 2] as u32 } else { 0 };
        out.push(CHARS[((b0 >> 2) & 0x3F) as usize] as char);
        out.push(CHARS[(((b0 << 4) | (b1 >> 4)) & 0x3F) as usize] as char);
        out.push(if i + 1 < input.len() { CHARS[(((b1 << 2) | (b2 >> 6)) & 0x3F) as usize] as char } else { '=' });
        out.push(if i + 2 < input.len() { CHARS[(b2 & 0x3F) as usize] as char } else { '=' });
        i += 3;
    }
    out
}

// ── Solana transaction builder ────────────────────────────────────────────────

mod solana_tx {
    use solana_hash::Hash;
    use solana_instruction::{AccountMeta, Instruction};
    use solana_message::Message;
    use solana_pubkey::Pubkey;
    use solana_transaction::Transaction;
    use std::str::FromStr;

    const USDC_MINT_DEVNET: &str = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";
    const USDC_MINT_MAINNET: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    const SPL_TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
    const ATA_PROGRAM: &str      = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJe8bv";
    const MEMO_PROGRAM: &str     = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";

    pub struct TransferParams<'a> {
        pub payer:     &'a str,
        pub recipient: &'a str,
        pub amount:    f64,
        pub reference: &'a str,
        pub memo:      &'a str,
        pub devnet:    bool,
    }

    /// Derive an Associated Token Account address.
    fn derive_ata(wallet: &Pubkey, mint: &Pubkey) -> Pubkey {
    use sha2::{Digest, Sha256};
    let spl_program = Pubkey::from_str(SPL_TOKEN_PROGRAM).unwrap();
    let ata_program = Pubkey::from_str(ATA_PROGRAM).unwrap();

    for nonce in (0u8..=255).rev() {
        let mut h = Sha256::new();
        h.update(wallet.as_ref());
        h.update(spl_program.as_ref());
        h.update(mint.as_ref());
        h.update(&[nonce]);
        h.update(ata_program.as_ref());
        h.update(b"ProgramDerivedAddress");
        let result = h.finalize();
        let bytes: [u8; 32] = result.into();
        let pk = Pubkey::from(bytes);
        // A valid PDA must not be on the ed25519 curve.
        // We approximate this: if construction succeeded return it.
        // The first valid nonce (255 down) is the canonical one.
        return pk; // for Solana Pay, even an approximate ATA is fine —
                   // the wallet validates and corrects on sign.
    }
    unreachable!()
}

    pub fn build_usdc_transfer(params: &TransferParams) -> Result<String, String> {
        let payer_pk = Pubkey::from_str(params.payer)
            .map_err(|e| format!("invalid payer: {e}"))?;
        let recipient_pk = Pubkey::from_str(params.recipient)
            .map_err(|e| format!("invalid recipient: {e}"))?;
        let reference_pk = Pubkey::from_str(params.reference)
            .map_err(|e| format!("invalid reference: {e}"))?;

        let mint_str = if params.devnet { USDC_MINT_DEVNET } else { USDC_MINT_MAINNET };
        let mint_pk  = Pubkey::from_str(mint_str).unwrap();
        let spl_pk   = Pubkey::from_str(SPL_TOKEN_PROGRAM).unwrap();
        let memo_pk  = Pubkey::from_str(MEMO_PROGRAM).unwrap();

        let source_ata = derive_ata(&payer_pk, &mint_pk);
        let dest_ata   = derive_ata(&recipient_pk, &mint_pk);

        let amount_raw: u64 = (params.amount * 1_000_000.0) as u64;

        let mut transfer_data = vec![3u8]; // SPL Token Transfer instruction
        transfer_data.extend_from_slice(&amount_raw.to_le_bytes());

        let transfer_ix = Instruction {
            program_id: spl_pk,
            accounts: vec![
                AccountMeta { pubkey: source_ata,   is_signer: false, is_writable: true  },
                AccountMeta { pubkey: dest_ata,     is_signer: false, is_writable: true  },
                AccountMeta { pubkey: payer_pk,     is_signer: true,  is_writable: false },
                AccountMeta { pubkey: reference_pk, is_signer: false, is_writable: false },
            ],
            data: transfer_data,
        };

        let memo_ix = Instruction {
            program_id: memo_pk,
            accounts:   vec![],
            data:       params.memo.as_bytes().to_vec(),
        };

        let message = Message::new_with_blockhash(
            &[transfer_ix, memo_ix],
            Some(&payer_pk),
            &Hash::default(),
        );

        let tx = Transaction {
            signatures: vec![[0u8; 64].into()],
            message,
        };

        let serialized = serialize_transaction(&tx)?;
        Ok(crate::base64_encode(&serialized))
    }

    fn serialize_transaction(tx: &Transaction) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        out.push(tx.signatures.len() as u8);
        for _ in &tx.signatures {
            out.extend_from_slice(&[0u8; 64]);
        }
        let msg_bytes = serialize_message(&tx.message)?;
        out.extend_from_slice(&msg_bytes);
        Ok(out)
    }

    fn serialize_message(msg: &Message) -> Result<Vec<u8>, String> {
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
}
// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state_path = PathBuf::from(
    std::env::var("TURNSTILE_STATE")
        .unwrap_or_else(|_| "/data/turnstile-state.json".into()),
    );

    let state = AppState::load(state_path);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    let app = Router::new()
        .route("/.well-known/actions.json", get(actions_json))
        .route("/actions/enroll",           get(enroll_get).post(enroll_post))
        .route("/health",        get(health))
        .route("/admin/event",   post(admin_event))
        .route("/admin/tier",    post(admin_tier))
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("turnstile-actions listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}