use crate::models::{
    board_api::{BoardAPI, Idle, InGame, PlayedBy, Turn},
    board_local::BoardLocal,
    chess::{ChessApp, ChessBackend},
    oauth::{LichessUser, TokenInfo},
};
use log::warn;

impl ChessApp {
    /// Constructs an online backend in the `Idle` state. No game is scoped
    /// yet — call `attach_game` after picking one from the ongoing-games list.
    pub fn new_online(token: TokenInfo, user: LichessUser) -> ChessApp {
        Self {
            backend: ChessBackend::OnlineIdle(BoardAPI::<Idle>::new(token, user)),
        }
    }

    pub fn new_offline() -> ChessApp {
        let board_local = BoardLocal::new(String::new());
        Self {
            backend: ChessBackend::Offline(board_local),
        }
    }

    /// Transition the underlying API from `Idle` to `InGame`. `my_turn` is the
    /// snapshot from `GameData.is_my_turn` so the sidebar can render an initial
    /// turn status before the game-state stream takes over.
    pub fn attach_game(mut self, game_id: String, my_turn: bool) -> Self {
        self.backend = match self.backend {
            ChessBackend::OnlineIdle(api) => {
                ChessBackend::OnlineInGame(api.attach_game(game_id, my_turn))
            }
            other => {
                warn!("attach_game called on non-idle backend; ignored");
                other
            }
        };
        self
    }

    /// Cloned `BoardAPI<Idle>` for use in tasks that only need read access to
    /// the auth scope (e.g. ongoing-games fetch).
    pub fn online_idle_api(&self) -> Option<BoardAPI<Idle>> {
        match &self.backend {
            ChessBackend::OnlineIdle(api) => Some(api.clone()),
            _ => None,
        }
    }

    /// Cloned `BoardAPI<InGame>` for use in tasks scoped to a specific game
    /// (game-state stream, move submission). The clone's runtime mutations
    /// stay inside the task — propagate state changes back via `AppEvent`s.
    pub fn online_in_game_api(&self) -> Option<BoardAPI<InGame>> {
        match &self.backend {
            ChessBackend::OnlineInGame(api) => Some(api.clone()),
            _ => None,
        }
    }

    pub fn turn(&self) -> Option<&Turn> {
        match &self.backend {
            ChessBackend::OnlineInGame(api) => Some(&api.state.turn),
            _ => None,
        }
    }

    /// The OAuth token backing this app, if any. Used to make authenticated
    /// `/api/puzzle/next` requests; an offline backend has no token and the
    /// puzzle screen then falls back to anonymous puzzle fetches.
    pub fn token(&self) -> Option<TokenInfo> {
        match &self.backend {
            ChessBackend::OnlineIdle(api) => Some(api.token.clone()),
            ChessBackend::OnlineInGame(api) => Some(api.token.clone()),
            ChessBackend::Offline(_) => None,
        }
    }

    /// Apply the initial `GameFull` snapshot to the in-game state. No-op for
    /// non-in-game backends.
    pub fn apply_game_full(
        &mut self,
        white: PlayedBy,
        black: PlayedBy,
        player0_white: bool,
        turn: Turn,
    ) {
        if let ChessBackend::OnlineInGame(api) = &mut self.backend {
            api.state.white = Some(white);
            api.state.black = Some(black);
            api.state.player0_white = player0_white;
            api.state.turn = turn;
        }
    }

    pub fn apply_turn(&mut self, turn: Turn) {
        if let ChessBackend::OnlineInGame(api) = &mut self.backend {
            api.state.turn = turn;
        }
    }
}
