use serde::{Deserialize, Serialize};

use crate::models::bitboard::{Bitboards, Color};
use crate::models::game::Game;

// ─── Wire types ───────────────────────────────────────────────────────────────
// Shape returned by GET /api/puzzle/daily and GET /api/puzzle/next. Both
// endpoints hand back the puzzle position as an explicit FEN plus the full UCI
// solution — a Lichess puzzle is a self-contained training position, *not* a
// live game, so there is no game-state stream to attach to. Everything needed
// to play a puzzle is in this one response; interaction is entirely local.

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Puzzle {
    pub id: String,
    /// Difficulty rating of the puzzle itself — surfaced in the sidebar.
    pub rating: i32,
    pub plays: i32,
    /// UCI moves. Even indices (0, 2, …) are the solver's moves; odd indices
    /// are the opponent's scripted replies that get auto-played.
    pub solution: Vec<String>,
    pub themes: Vec<String>,
    #[serde(rename = "initialPly")]
    pub initial_ply: i32,
    /// The exact puzzle position; the side-to-move field is the side the solver
    /// plays. Present on `/puzzle/daily` and `/puzzle/{id}`, but **omitted by
    /// `/puzzle/next`** — which is why `get_next_puzzle` re-fetches by id.
    #[serde(default)]
    pub fen: Option<String>,
    /// The opponent's setup move (UCI) that created the puzzle — drives the
    /// initial last-move highlight. Omitted by `/puzzle/next`, absent on a few
    /// legacy puzzles.
    #[serde(rename = "lastMove", default)]
    pub last_move: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct PuzzleResponse {
    // The source game the puzzle was lifted from. Parsed for completeness but
    // never used — the puzzle plays entirely from `puzzle.fen`.
    #[allow(dead_code)]
    pub game: Game,
    pub puzzle: Puzzle,
}

// ─── Runtime session ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum PuzzleStatus {
    /// Waiting for the solver's move — the initial state, and the state after a
    /// correct non-final move once the opponent's reply has been auto-played.
    Solving,
    /// The solver just played a correct, non-final move.
    Correct,
    /// The solver played a wrong move; the position was left untouched.
    Wrong,
    /// Every solution move has been played.
    Solved,
}

/// Live state of a puzzle being solved. Built once from a `Puzzle` and mutated
/// in place as the solver makes moves.
pub struct PuzzleSession {
    pub rating: i32,
    pub themes: Vec<String>,
    /// Live board position. Starts at the puzzle FEN and advances as correct
    /// moves (the solver's plus the auto-played opponent replies) land.
    pub position: Bitboards,
    pub solution: Vec<String>,
    /// Index of the next move expected from the solver. Even while solving,
    /// equals `solution.len()` once solved.
    pub progress: usize,
    /// True when the solver plays White (= side-to-move in the puzzle FEN).
    pub solver_white: bool,
    pub status: PuzzleStatus,
    /// Square bitmask of the most recent move, for the board's highlight.
    pub last_move_mask: u64,
}

impl PuzzleSession {
    /// Build a session from a fetched puzzle. Fails if the puzzle has no FEN
    /// (a raw `/puzzle/next` record — callers must re-fetch by id first) or the
    /// FEN is unparseable. Every other field is taken at face value.
    pub fn new(puzzle: Puzzle) -> Result<Self, String> {
        let fen = puzzle
            .fen
            .as_deref()
            .ok_or_else(|| "puzzle response carried no FEN".to_string())?;
        let position = Bitboards::from_fen(fen)?;
        let solver_white = position.side_to_move == Color::White;
        let last_move_mask = puzzle
            .last_move
            .as_deref()
            .map(uci_from_to_mask)
            .unwrap_or(0);
        Ok(Self {
            rating: puzzle.rating,
            themes: puzzle.themes,
            position,
            solution: puzzle.solution,
            progress: 0,
            solver_white,
            status: PuzzleStatus::Solving,
            last_move_mask,
        })
    }

    /// Whether the solver is allowed to make a move. False while a correct
    /// move is awaiting its scripted opponent reply, and once solved.
    pub fn accepts_input(&self) -> bool {
        matches!(self.status, PuzzleStatus::Solving | PuzzleStatus::Wrong)
            && self.progress < self.solution.len()
    }

    /// UCI of the move the solver is expected to play next — used by the hint
    /// button to mark the piece that should move. `None` once the puzzle no
    /// longer accepts input.
    pub fn next_expected_move(&self) -> Option<&str> {
        if !self.accepts_input() {
            return None;
        }
        self.solution.get(self.progress).map(String::as_str)
    }

    /// Validate the solver's move (a UCI string) against the scripted solution.
    /// On a correct move only the solver's move is applied — the opponent's
    /// scripted reply is left *pending* (see `opponent_reply_pending` /
    /// `apply_opponent_reply`) so the caller can play it after a short delay
    /// rather than having it snap in instantly. On a wrong move `position` is
    /// left untouched so the board simply stays put. Returns the new status.
    pub fn try_move(&mut self, uci: &str) -> PuzzleStatus {
        if !self.accepts_input() {
            return self.status.clone();
        }
        let expected = self.solution[self.progress].clone();
        // Match on the from/to squares only (first 4 chars). The canonical
        // solution move is what actually gets applied, so an under-promotion
        // the user enters as a queen (the board widget's default) is still
        // accepted as correct.
        if uci.len() < 4
            || expected.len() < 4
            || uci.as_bytes()[..4] != expected.as_bytes()[..4]
        {
            self.status = PuzzleStatus::Wrong;
            return self.status.clone();
        }

        self.apply(&expected);
        self.progress += 1;

        self.status = if self.progress >= self.solution.len() {
            // The solver's move finished the puzzle — no opponent reply.
            PuzzleStatus::Solved
        } else {
            // A correct, non-final move — the opponent still owes a scripted
            // reply, played later via `apply_opponent_reply`.
            PuzzleStatus::Correct
        };
        self.status.clone()
    }

    /// Whether the opponent has a scripted reply queued — true exactly after
    /// `try_move` returned `Correct`. The caller plays it with
    /// `apply_opponent_reply`.
    pub fn opponent_reply_pending(&self) -> bool {
        self.status == PuzzleStatus::Correct && self.progress < self.solution.len()
    }

    /// Apply the opponent's scripted reply to `position`. Intended to be called
    /// after a delay once `try_move` returned `Correct`; a no-op if no reply is
    /// pending. Returns the new status — `Solving` (solver to move again) or
    /// `Solved` (the reply was the puzzle's last move).
    pub fn apply_opponent_reply(&mut self) -> PuzzleStatus {
        if !self.opponent_reply_pending() {
            return self.status.clone();
        }
        let reply = self.solution[self.progress].clone();
        self.apply(&reply);
        self.progress += 1;
        self.status = if self.progress >= self.solution.len() {
            PuzzleStatus::Solved
        } else {
            PuzzleStatus::Solving
        };
        self.status.clone()
    }

    /// Apply one UCI move to the live position and refresh the last-move mask.
    fn apply(&mut self, uci: &str) {
        let before = self.position.clone();
        if let Err(e) = self.position.apply_uci_move(uci) {
            log::warn!("Puzzle move '{}' failed to apply: {}", uci, e);
            return;
        }
        self.last_move_mask = diff_mask(&before, &self.position);
    }
}

/// Bitmask of squares whose occupant differs between two positions. Catches
/// castling rook squares and en-passant captures without special-casing UCI.
fn diff_mask(a: &Bitboards, b: &Bitboards) -> u64 {
    let mut mask = 0u64;
    for sq in 0..64u8 {
        if a.piece_at(sq) != b.piece_at(sq) {
            mask |= 1u64 << sq;
        }
    }
    mask
}

/// Bitmask of the from + to squares of a UCI move. Used only for the puzzle's
/// initial last-move highlight, where the pre-move position isn't available to
/// diff against.
fn uci_from_to_mask(uci: &str) -> u64 {
    let b = uci.as_bytes();
    if b.len() < 4 {
        return 0;
    }
    let mut mask = 0u64;
    for (file, rank) in [(b[0], b[1]), (b[2], b[3])] {
        if (b'a'..=b'h').contains(&file) && (b'1'..=b'8').contains(&rank) {
            let sq = (rank - b'1') * 8 + (file - b'a');
            mask |= 1u64 << sq;
        }
    }
    mask
}

// ─── /api/puzzle/next parameters ──────────────────────────────────────────────
// Each enum exposes `label` (for the settings UI), the wire value, and `next`
// for the tap-to-cycle settings buttons.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Difficulty {
    Easiest,
    Easier,
    Normal,
    Harder,
    Hardest,
}

impl Difficulty {
    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Easiest => "Easiest",
            Difficulty::Easier => "Easier",
            Difficulty::Normal => "Normal",
            Difficulty::Harder => "Harder",
            Difficulty::Hardest => "Hardest",
        }
    }

    pub fn as_param(self) -> &'static str {
        match self {
            Difficulty::Easiest => "easiest",
            Difficulty::Easier => "easier",
            Difficulty::Normal => "normal",
            Difficulty::Harder => "harder",
            Difficulty::Hardest => "hardest",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Difficulty::Easiest => Difficulty::Easier,
            Difficulty::Easier => Difficulty::Normal,
            Difficulty::Normal => Difficulty::Harder,
            Difficulty::Harder => Difficulty::Hardest,
            Difficulty::Hardest => Difficulty::Easiest,
        }
    }
}

/// Game phase, mapped to a Lichess puzzle theme key for the `angle` parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PuzzlePhase {
    Any,
    Opening,
    Middlegame,
    Endgame,
    RookEndgame,
    BishopEndgame,
    PawnEndgame,
    KnightEndgame,
    QueenEndgame,
    QueenRookEndgame,
}

impl PuzzlePhase {
    pub fn label(self) -> &'static str {
        match self {
            PuzzlePhase::Any => "Any",
            PuzzlePhase::Opening => "Opening",
            PuzzlePhase::Middlegame => "Middlegame",
            PuzzlePhase::Endgame => "Endgame",
            PuzzlePhase::RookEndgame => "Rook endgame",
            PuzzlePhase::BishopEndgame => "Bishop endgame",
            PuzzlePhase::PawnEndgame => "Pawn endgame",
            PuzzlePhase::KnightEndgame => "Knight endgame",
            PuzzlePhase::QueenEndgame => "Queen endgame",
            PuzzlePhase::QueenRookEndgame => "Queen & rook",
        }
    }

    /// The Lichess puzzle theme key for the `angle` query parameter, or `None`
    /// for "Any" (the parameter is then omitted entirely).
    pub fn as_angle(self) -> Option<&'static str> {
        match self {
            PuzzlePhase::Any => None,
            PuzzlePhase::Opening => Some("opening"),
            PuzzlePhase::Middlegame => Some("middlegame"),
            PuzzlePhase::Endgame => Some("endgame"),
            PuzzlePhase::RookEndgame => Some("rookEndgame"),
            PuzzlePhase::BishopEndgame => Some("bishopEndgame"),
            PuzzlePhase::PawnEndgame => Some("pawnEndgame"),
            PuzzlePhase::KnightEndgame => Some("knightEndgame"),
            PuzzlePhase::QueenEndgame => Some("queenEndgame"),
            PuzzlePhase::QueenRookEndgame => Some("queenRookEndgame"),
        }
    }

    pub fn next(self) -> Self {
        match self {
            PuzzlePhase::Any => PuzzlePhase::Opening,
            PuzzlePhase::Opening => PuzzlePhase::Middlegame,
            PuzzlePhase::Middlegame => PuzzlePhase::Endgame,
            PuzzlePhase::Endgame => PuzzlePhase::RookEndgame,
            PuzzlePhase::RookEndgame => PuzzlePhase::BishopEndgame,
            PuzzlePhase::BishopEndgame => PuzzlePhase::PawnEndgame,
            PuzzlePhase::PawnEndgame => PuzzlePhase::KnightEndgame,
            PuzzlePhase::KnightEndgame => PuzzlePhase::QueenEndgame,
            PuzzlePhase::QueenEndgame => PuzzlePhase::QueenRookEndgame,
            PuzzlePhase::QueenRookEndgame => PuzzlePhase::Any,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PuzzleColor {
    Random,
    White,
    Black,
}

impl PuzzleColor {
    pub fn label(self) -> &'static str {
        match self {
            PuzzleColor::Random => "Random",
            PuzzleColor::White => "White",
            PuzzleColor::Black => "Black",
        }
    }

    /// Wire value for the `color` query parameter, or `None` for a random
    /// colour (the parameter is then omitted).
    pub fn as_param(self) -> Option<&'static str> {
        match self {
            PuzzleColor::Random => None,
            PuzzleColor::White => Some("white"),
            PuzzleColor::Black => Some("black"),
        }
    }

    pub fn next(self) -> Self {
        match self {
            PuzzleColor::Random => PuzzleColor::White,
            PuzzleColor::White => PuzzleColor::Black,
            PuzzleColor::Black => PuzzleColor::Random,
        }
    }
}

/// The full set of `/api/puzzle/next` request parameters chosen on the puzzle
/// settings screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PuzzleParams {
    pub difficulty: Difficulty,
    pub phase: PuzzlePhase,
    pub color: PuzzleColor,
}

impl Default for PuzzleParams {
    fn default() -> Self {
        Self {
            difficulty: Difficulty::Normal,
            phase: PuzzlePhase::Any,
            color: PuzzleColor::Random,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::bitboard::{square, Color, Piece};

    fn puzzle(fen: &str, solution: &[&str], last_move: Option<&str>) -> Puzzle {
        Puzzle {
            id: "test".to_string(),
            rating: 1500,
            plays: 0,
            solution: solution.iter().map(|s| s.to_string()).collect(),
            themes: vec![],
            initial_ply: 0,
            fen: Some(fen.to_string()),
            last_move: last_move.map(|s| s.to_string()),
        }
    }

    #[test]
    fn solver_color_from_fen() {
        // Side-to-move "b" in the FEN means the solver plays Black.
        let s = PuzzleSession::new(puzzle(
            "4k2r/4bppp/1qpB4/1p2p3/4Q1n1/5N2/PPP2PPP/R2R2K1 b k - 0 1",
            &["g4f2", "e4d4", "e5d4"],
            Some("d3e4"),
        ))
        .unwrap();
        assert!(!s.solver_white);
        assert_eq!(s.status, PuzzleStatus::Solving);
    }

    #[test]
    fn correct_sequence_solves_puzzle() {
        let mut s = PuzzleSession::new(puzzle(
            "4k2r/4bppp/1qpB4/1p2p3/4Q1n1/5N2/PPP2PPP/R2R2K1 b k - 0 1",
            &["g4f2", "e4d4", "e5d4"],
            Some("d3e4"),
        ))
        .unwrap();
        // A correct, non-final move leaves the opponent's reply pending and
        // blocks further input until it is played.
        assert_eq!(s.try_move("g4f2"), PuzzleStatus::Correct);
        assert!(s.opponent_reply_pending());
        assert!(!s.accepts_input());
        // Playing the reply (e4d4) advances the position and hands control back.
        assert_eq!(s.apply_opponent_reply(), PuzzleStatus::Solving);
        assert_eq!(s.position.piece_at(square(3, 3)), Some((Color::White, Piece::Queen)));
        assert!(s.accepts_input());
        // Final move finishes the puzzle.
        assert_eq!(s.try_move("e5d4"), PuzzleStatus::Solved);
        assert!(!s.accepts_input());
    }

    #[test]
    fn wrong_move_leaves_position_and_allows_retry() {
        let mut s = PuzzleSession::new(puzzle(
            "4k2r/4bppp/1qpB4/1p2p3/4Q1n1/5N2/PPP2PPP/R2R2K1 b k - 0 1",
            &["g4f2", "e4d4", "e5d4"],
            Some("d3e4"),
        ))
        .unwrap();
        let before = s.position.clone();
        assert_eq!(s.try_move("g4h2"), PuzzleStatus::Wrong);
        // Position untouched after a wrong move.
        assert_eq!(s.position.piece_at(square(7, 1)), before.piece_at(square(7, 1)));
        assert_eq!(s.progress, 0);
        // Retrying with the right move still works.
        assert_eq!(s.try_move("g4f2"), PuzzleStatus::Correct);
    }

    #[test]
    fn underpromotion_accepted_via_from_to_match() {
        // Solution is an under-promotion to a knight; the board widget always
        // submits a queen suffix, so try_move must match on from/to and apply
        // the canonical solution move.
        let mut s = PuzzleSession::new(puzzle(
            "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
            &["a7a8n"],
            None,
        ))
        .unwrap();
        assert_eq!(s.try_move("a7a8q"), PuzzleStatus::Solved);
        assert_eq!(
            s.position.piece_at(square(0, 7)),
            Some((Color::White, Piece::Knight))
        );
    }
}
