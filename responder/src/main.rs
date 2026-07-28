use axum::{
    Router,
    extract::{Query, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Json},
    routing::{get},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::SocketAddr,
    path::PathBuf,
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

    Json(ActionPostResponse {
        transaction: build_placeholder_tx(&body.account, &tier, &reference_key),
        message,
    })
    .into_response()
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

fn build_placeholder_tx(account: &str, tier: &PriceTier, reference: &str) -> String {
    let stub = serde_json::json!({
        "type": "solana_pay_transfer",
        "recipient": "RECIPIENT_PUBKEY_PLACEHOLDER",
        "amount": tier.amount_usdc,
        "spl_token": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
        "reference": reference,
        "payer": account,
        "memo": "Turnstile enrollment"
    });
    let json_bytes = stub.to_string().into_bytes();
    base64_encode(&json_bytes)
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

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state_path = PathBuf::from(
        std::env::var("TURNSTILE_STATE").unwrap_or_else(|_| "turnstile-state.json".into()),
    );

    let state = AppState::load(state_path);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    let app = Router::new()
        .route("/.well-known/actions.json", get(actions_json))
        .route("/actions/enroll",           get(enroll_get).post(enroll_post))
        .route("/health",                   get(health))
        .layer(cors)
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