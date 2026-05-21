use image::{ImageBuffer, Luma};
use std::sync::Arc;
use std::time::Duration;
use x11rb::protocol::xproto;

use crate::api::github::UpdateInfo;
use crate::models::{
    bitboard::Bitboards,
    board_api::{GameDataList, PlayedBy, Turn},
    chess::ChessApp,
    oauth::{LichessUser, TokenInfo},
    puzzle::{Puzzle, PuzzleParams},
};

#[derive(Debug, Clone)]
pub enum AppEvent {
    // Authentication Events
    AuthSuccess(TokenInfo, LichessUser),
    AuthFailed(String),
    QrReady(ImageBuffer<Luma<u8>, Vec<u8>>),

    // Ongoing-games fetch
    OngoingGamesLoaded(Arc<GameDataList>),
    OngoingGamesFailed(String),

    // Update flow → UpdateScreen.
    // - Available: a strictly newer release was found, with verified asset metadata.
    // - UpToDate:  GitHub responded but no newer release exists.
    // - CheckFailed: the GitHub query itself errored (network, parse, rate limit).
    // - Applied / ApplyFailed: result of the download+verify+swap apply path.
    UpdateAvailable(UpdateInfo),
    UpdateUpToDate,
    UpdateCheckFailed(String),
    UpdateApplied,
    UpdateApplyFailed(String),

    // Game-state stream → ChessGameScreen. Emitted from the spawned stream
    // task; the screen uses them to update its own ChessApp copy, the board
    // widget, and the sidebar (mutations to the task's local clone don't
    // propagate). `board` is the position derived from `initial_fen` + the
    // event's full move list — the screen replaces the widget's bitboard
    // wholesale on every update.
    GameFullReceived {
        white: PlayedBy,
        black: PlayedBy,
        player0_white: bool,
        turn: Turn,
        board: Bitboards,
        // Bitmask of squares affected by the last UCI move in the stream's
        // move list (from + to, plus the rook squares for a castle, plus any
        // captured-pawn square for en passant). 0 when there is no last move
        // (fresh game). Drives the board widget's last-move corner-bracket
        // highlight.
        last_move: u64,
    },
    TurnChanged {
        turn: Turn,
        board: Bitboards,
        last_move: u64,
    },

    // Puzzle flow → PuzzleScreen / PuzzleSettingsScreen.
    // - PuzzleLoaded: a daily/next puzzle was fetched and parsed.
    // - PuzzleLoadFailed: the fetch or parse errored.
    // - OpenPuzzleSettings / NextPuzzle / ShowPuzzleHint: emitted by the puzzle
    //   sidebar buttons.
    // - ApplyPuzzleParams: re-emitted by the settings screen on its way out,
    //   carrying the chosen params back to PuzzleScreen to drive a /next fetch.
    PuzzleLoaded(Puzzle),
    PuzzleLoadFailed(String),
    OpenPuzzleSettings,
    NextPuzzle,
    ShowPuzzleHint,
    // The opponent's scripted reply, delivered ~1 s after a correct move so it
    // doesn't snap in instantly (posted by a delayed task — see PuzzleScreen).
    PuzzleOpponentMove,
    ApplyPuzzleParams(PuzzleParams),

    // UI Events
    Touch(TouchEvent),
    Redraw,
    Tick(Duration),

    // Chess Events
    MoveMade(ChessMove),
    SquareSelected(Square),
    // The server rejected the last move (illegal — e.g. leaves the king in
    // check). Posted by the move-submission task back to ChessGameScreen.
    MoveRejected,

    // Navigation
    ExitToMenu,
    // Open the live-game actions screen (resign / abort).
    OpenGameActions,
    ChessReady(ChessApp),
    Quit,

    // X11 Events
    Expose,
    WindowUnmapped,
}

#[derive(Debug, Clone, Copy)]
pub struct TouchEvent {
    pub x: i16,
    pub y: i16,
    pub kind: TouchKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TouchKind {
    Down,
    Up,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Square {
    pub file: u8, // 0-7 (a-h)
    pub rank: u8, // 0-7 (1-8)
}

impl Square {
    pub fn new(file: u8, rank: u8) -> Self {
        Self { file, rank }
    }

    pub fn to_algebraic(&self) -> String {
        format!("{}{}", (b'a' + self.file) as char, self.rank + 1)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ChessMove {
    pub from: Square,
    pub to: Square,
}

pub type Rectangle = xproto::Rectangle;

pub trait RectangleExt {
    fn new(x: i16, y: i16, width: u16, height: u16) -> Self;
    fn contains(&self, px: i16, py: i16) -> bool;
}

impl RectangleExt for Rectangle {
    fn new(x: i16, y: i16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn contains(&self, px: i16, py: i16) -> bool {
        px >= self.x
            && px < self.x + self.width as i16
            && py >= self.y
            && py < self.y + self.height as i16
    }
}
