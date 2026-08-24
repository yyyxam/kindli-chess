use crate::models::oauth::TokenInfo;
use crate::models::puzzle::{Puzzle, PuzzleParams, PuzzleResponse};

use log::{info, warn};
use reqwest::Url;
use serde::Deserialize;

// ~~~~~~~~~~~~~~~~ PUZZLE-ENDPOINTS ~~~~~~~~~~~~~~~~

/// GET /api/puzzle/daily — the daily puzzle. No authentication required. This
/// endpoint includes the puzzle `fen` directly.
pub async fn get_daily_puzzle() -> Result<Puzzle, Box<dyn std::error::Error>> {
    let url = Url::parse(&format!("{}/puzzle/daily", env!("LICHESS_API_BASE")))?;
    let response: PuzzleResponse = reqwest::get(url).await?.json().await?;
    Ok(response.puzzle)
}

/// GET /api/puzzle/{id} — a puzzle by id. No authentication required. Unlike
/// `/puzzle/next`, this endpoint includes the `fen` and `lastMove` fields.
pub async fn get_puzzle_by_id(id: &str) -> Result<Puzzle, Box<dyn std::error::Error>> {
    let url = Url::parse(&format!("{}/puzzle/{}", env!("LICHESS_API_BASE"), id))?;
    let response: PuzzleResponse = reqwest::get(url).await?.json().await?;
    Ok(response.puzzle)
}

/// GET /api/puzzle/next — a fresh puzzle matching `params`.
///
/// Authentication is *optional*: with a token Lichess tailors the puzzle to the
/// user's rating and skips ones they've solved. But that path needs the
/// `puzzle:read` OAuth scope — tokens issued before that scope was requested
/// get a 403, so a failed authenticated request transparently retries
/// anonymously.
///
/// Note: `/puzzle/next` itself omits the `fen` / `lastMove` fields, so we take
/// only the puzzle id from it and re-fetch the complete record by id.
pub async fn get_next_puzzle(
    token: Option<TokenInfo>,
    params: PuzzleParams,
) -> Result<Puzzle, Box<dyn std::error::Error>> {
    let url = build_next_url(&params)?;

    let id = match &token {
        Some(token) => {
            // Map the error to a String right away: a non-Send `Box<dyn Error>`
            // held across the retry's await would make this future non-Send,
            // and it is spawned onto the runtime.
            let authed = fetch_next_id(url.clone(), Some(token))
                .await
                .map_err(|e| e.to_string());
            match authed {
                Ok(id) => id,
                Err(msg) => {
                    warn!("Authenticated puzzle/next failed ({msg}); retrying anonymously");
                    fetch_next_id(url, None).await?
                }
            }
        }
        None => fetch_next_id(url, None).await?,
    };

    get_puzzle_by_id(&id).await
}

/// Build the `/api/puzzle/next` URL with the difficulty / angle / color query
/// parameters set from `params`.
fn build_next_url(params: &PuzzleParams) -> Result<Url, Box<dyn std::error::Error>> {
    let mut url = Url::parse(&format!("{}/puzzle/next", env!("LICHESS_API_BASE")))?;
    {
        // Scope the mutable query borrow so `url` is free to return afterwards.
        let mut query = url.query_pairs_mut();
        query.append_pair("difficulty", params.difficulty.as_param());
        if let Some(angle) = params.phase.as_angle() {
            query.append_pair("angle", angle);
        }
        if let Some(color) = params.color.as_param() {
            query.append_pair("color", color);
        }
    }
    Ok(url)
}

/// Perform one `/puzzle/next` request (optionally authenticated) and return the
/// selected puzzle's id.
async fn fetch_next_id(
    url: Url,
    token: Option<&TokenInfo>,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut request = reqwest::Client::new().get(url);
    if let Some(token) = token {
        request = request.bearer_auth(&token.access_token);
    }

    let response = request.send().await?;
    if !response.status().is_success() {
        return Err(format!("puzzle/next request failed: {}", response.status()).into());
    }

    let response: PuzzleResponse = response.json().await?;
    Ok(response.puzzle.id)
}

// ~~~~~~~~~~~~~~~~ HOME-SCREEN PUZZLE SELECTION ~~~~~~~~~~~~~~~~

/// Pick the puzzle to open from the home screen's Puzzle button.
///
/// Normally this is the daily puzzle. But if the signed-in account has already
/// played today's daily, a fresh `/puzzle/next` puzzle is returned instead so
/// the user isn't handed something they just solved.
///
/// The "already played" check needs the `puzzle:read` scope. With no token, or
/// when the check itself errors (missing scope, network), it degrades quietly
/// to the daily puzzle.
pub async fn get_home_puzzle(
    token: Option<TokenInfo>,
    params: PuzzleParams,
) -> Result<Puzzle, Box<dyn std::error::Error>> {
    let daily = get_daily_puzzle().await?;

    if let Some(token) = &token {
        match daily_already_played(token, &daily.id).await {
            Ok(true) => {
                info!(
                    "Daily puzzle {} already played — fetching a fresh puzzle",
                    daily.id
                );
                return get_next_puzzle(Some(token.clone()), params).await;
            }
            Ok(false) => {}
            Err(msg) => warn!("Daily-played check failed ({msg}); showing the daily puzzle"),
        }
    }

    Ok(daily)
}

/// One `/api/puzzle/activity` NDJSON record. Only the puzzle id is needed; the
/// `date` / `win` / remaining `puzzle` fields are ignored.
#[derive(Deserialize)]
struct PuzzleActivityRecord {
    puzzle: PuzzleActivityPuzzle,
}

#[derive(Deserialize)]
struct PuzzleActivityPuzzle {
    id: String,
}

/// Whether `daily_id` appears in the account's recent puzzle activity — i.e.
/// the account has already played today's daily puzzle (won or lost; either
/// way they've seen it). Scans the most recent `ACTIVITY_SCAN` records.
/// Requires the `puzzle:read` scope.
async fn daily_already_played(token: &TokenInfo, daily_id: &str) -> Result<bool, String> {
    // The daily puzzle resets every 24 h, so a user who played it has it within
    // their last handful of attempts unless they then solved dozens more — 50
    // records covers every realistic case in one request.
    const ACTIVITY_SCAN: u32 = 50;

    let url = format!(
        "{}/puzzle/activity?max={}",
        env!("LICHESS_API_BASE"),
        ACTIVITY_SCAN
    );
    let response = reqwest::Client::new()
        .get(&url)
        .bearer_auth(&token.access_token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "puzzle/activity request failed: {}",
            response.status()
        ));
    }
    let body = response.text().await.map_err(|e| e.to_string())?;

    // NDJSON: one activity record per line. Lines that fail to parse are
    // skipped rather than failing the whole check.
    Ok(body
        .lines()
        .filter_map(|line| serde_json::from_str::<PuzzleActivityRecord>(line.trim()).ok())
        .any(|record| record.puzzle.id == daily_id))
}
