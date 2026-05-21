use crate::models::bitboard::{Bitboards, Color, Piece};
use crate::ui::events::{
    AppEvent, ChessMove, Rectangle, RectangleExt, Square, TouchEvent, TouchKind,
};
use crate::ui::renderer::{DrawColor, Renderer};
use image::{ImageBuffer, Rgba};
use log::{info, warn};
use std::path::Path;

const SQUARE_SIZE: u16 = 134; // 1072 / 8
// Source PNGs are 128×128, drawn at 96 (≈ 80 % of the 120 we used before)
// to free up corner real estate for the selection diagonals and last-move
// brackets without crowding the piece glyph.
const PIECE_DRAW_SIZE: u16 = 96;
// Length of every corner decoration along a square edge — an eighth of the
// edge in either direction. The diagonal selection cut is the hypotenuse of
// the same SEGMENT_LEN × SEGMENT_LEN triangle, so its length naturally lands
// at an eighth of the square's diagonal.
const SEGMENT_LEN: i16 = SQUARE_SIZE as i16 / 8;
// Stroke width of the selection diagonals (px). Thick enough to read on
// e-ink at typical viewing distance.
const SELECTION_STROKE: u16 = 4;
// Stroke width of the last-move corner brackets (px). A touch thicker than
// the selection so the two decorations are distinguishable when both apply
// to the same square.
const LAST_MOVE_STROKE: i16 = 6;

pub struct BoardWidget {
    area: Rectangle,
    selected_square: Option<Square>,
    flipped: bool, // View from black's perspective
    /// Current position to render. `None` until the first
    /// `GameFullReceived` / `TurnChanged` event lands.
    position: Option<Bitboards>,
    /// Last position actually painted to the screen. `render` diffs this
    /// against `position` and only repaints squares whose contents changed
    /// — this is what stops the whole board flashing on every server event.
    last_drawn_position: Option<Bitboards>,
    /// Last selection actually painted. Diffed against `selected_square` so
    /// taps repaint only the affected squares instead of the whole board.
    last_drawn_selection: Option<Square>,
    /// Bitmask (1 bit per square, LERF — see bitboard.rs) of squares that
    /// are part of the most-recently-played move on the wire. Drives the
    /// last-move corner-bracket highlight. Updated by `set_last_move`,
    /// rendered diff-style against `last_drawn_last_move`.
    last_move_squares: u64,
    last_drawn_last_move: u64,
    /// Force a full repaint on the next `render` call. Set only by changes
    /// that invalidate the entire board (orientation flip, first paint).
    /// Position and selection updates both go through the partial path.
    force_full_repaint: bool,
    /// Pre-decoded piece sprites, indexed by [color_idx][piece_idx] matching
    /// the order in `Bitboards::boards`. Loaded once on `new()`.
    piece_sprites: PieceSprites,
}

type Sprite = ImageBuffer<Rgba<u8>, Vec<u8>>;

struct PieceSprites {
    sprites: [[Option<Sprite>; 6]; 2],
}

impl PieceSprites {
    fn load() -> Self {
        let mut sprites: [[Option<Sprite>; 6]; 2] = Default::default();
        let assets = env!("ASSETS_DIR");
        for (color, color_dir) in [(Color::White, 0), (Color::Black, 1)] {
            for (piece, piece_idx, stem) in [
                (Piece::Pawn, 0, "chess-pawn"),
                (Piece::Knight, 1, "chess-knight"),
                (Piece::Bishop, 2, "chess-bishop"),
                (Piece::Rook, 3, "chess-rook"),
                (Piece::Queen, 4, "chess-queen"),
                (Piece::King, 5, "chess-king"),
            ] {
                // White pieces use the `_w.png` suffix, black pieces the bare
                // name — see CLAUDE.md and the assets directory.
                let suffix = if color == Color::White { "_w" } else { "" };
                let path = format!("{}{}{}.png", assets, stem, suffix);
                match image::open(Path::new(&path)) {
                    Ok(img) => {
                        sprites[color_dir][piece_idx] = Some(img.to_rgba8());
                    }
                    Err(e) => {
                        warn!(
                            "Failed to load piece sprite {} ({:?} {:?}): {}",
                            path, color, piece, e
                        );
                    }
                }
            }
        }
        Self { sprites }
    }

    fn get(&self, color: Color, piece: Piece) -> Option<&Sprite> {
        let c = match color {
            Color::White => 0,
            Color::Black => 1,
        };
        let p = match piece {
            Piece::Pawn => 0,
            Piece::Knight => 1,
            Piece::Bishop => 2,
            Piece::Rook => 3,
            Piece::Queen => 4,
            Piece::King => 5,
        };
        self.sprites[c][p].as_ref()
    }
}

impl BoardWidget {
    pub fn new(area: Rectangle) -> Self {
        Self {
            area,
            selected_square: None,
            flipped: false,
            position: None,
            last_drawn_position: None,
            last_drawn_selection: None,
            last_move_squares: 0,
            last_drawn_last_move: 0,
            force_full_repaint: false,
            piece_sprites: PieceSprites::load(),
        }
    }

    pub fn set_position(&mut self, board: Bitboards) {
        self.position = Some(board);
    }

    /// Set the squares to highlight as "last move played" — bitmask matches
    /// the LERF layout of `Bitboards`. Pass 0 to clear. Already-equal values
    /// are a no-op (no e-ink repaint), so it's safe to call on every event.
    pub fn set_last_move(&mut self, squares: u64) {
        self.last_move_squares = squares;
    }

    /// Programmatically set (or clear, with `None`) the selected-square
    /// highlight. Taps manage the selection themselves; this is for callers
    /// that mark a square without a tap — the puzzle hint button uses it to
    /// flag the piece that should move next. The square is in true board
    /// coordinates, the same as `handle_touch` produces.
    pub fn select_square(&mut self, square: Option<Square>) {
        self.selected_square = square;
    }

    pub fn set_flipped(&mut self, flipped: bool) {
        if self.flipped != flipped {
            self.flipped = flipped;
            self.force_full_repaint = true;
        }
    }

    /// Force the next `render` to fully repaint instead of diffing against the
    /// last-drawn position. Required whenever something outside the widget has
    /// drawn over the board area (e.g. another screen was shown on top), since
    /// the partial-render path assumes the framebuffer still holds what it
    /// last painted.
    pub fn invalidate(&mut self) {
        self.force_full_repaint = true;
    }

    pub fn handle_touch(&mut self, touch: &TouchEvent) -> Option<AppEvent> {
        if !self.area.contains(touch.x, touch.y) {
            return None;
        }

        // Convert to board coordinates
        let board_x = touch.x - self.area.x;
        let board_y = touch.y - self.area.y;

        let file = (board_x / SQUARE_SIZE as i16) as u8;
        let rank = 7 - (board_y / SQUARE_SIZE as i16) as u8;

        if file > 7 || rank > 7 {
            return None;
        }

        let square = Square::new(
            if self.flipped { 7 - file } else { file },
            if self.flipped { 7 - rank } else { rank },
        );

        if touch.kind == TouchKind::Down {
            info!("Board touched at {}", square.to_algebraic());

            if let Some(selected) = self.selected_square {
                if selected != square {
                    let chess_move = ChessMove {
                        from: selected,
                        to: square,
                    };
                    self.selected_square = None;
                    return Some(AppEvent::MoveMade(chess_move));
                } else {
                    // Tap the same square again → deselect.
                    self.selected_square = None;
                }
            } else {
                self.selected_square = Some(square);
                return Some(AppEvent::SquareSelected(square));
            }
        }

        None
    }

    pub fn render(&mut self, renderer: &mut Renderer) -> Result<(), Box<dyn std::error::Error>> {
        let do_partial = !self.force_full_repaint
            && self.last_drawn_position.is_some()
            && self.position.is_some();

        // Bitboard of squares the position-diff repainted this frame, so the
        // selection-diff step below knows which squares it can skip cleaning
        // (already clean) and which it needs to re-stamp the highlight onto.
        let mut repainted: u64 = 0;

        if do_partial {
            // Position diff: repaint only squares whose contents differ.
            let prev = self.last_drawn_position.as_ref().unwrap().clone();
            let curr = self.position.as_ref().unwrap().clone();
            for sq in 0..64u8 {
                let before = prev.piece_at(sq);
                let after = curr.piece_at(sq);
                if before != after {
                    self.repaint_square(renderer, sq, after, before.is_some())?;
                    repainted |= 1u64 << sq;
                }
            }
        } else {
            // Full paint: every square + every piece + border.
            for rank in 0..8u8 {
                for file in 0..8u8 {
                    // Square color is intrinsic to (file, rank) — a1 is dark
                    // regardless of orientation (standard "white square on
                    // the right" → h1 light, a1 dark, which is even-sum dark).
                    // Display coords are flipped separately so the same
                    // true-coord square ends up on the other side of the
                    // screen when viewing as black.
                    let is_dark = (rank + file) % 2 == 0;
                    let bg = if is_dark {
                        DrawColor::DarkGray
                    } else {
                        DrawColor::LightGray
                    };
                    let display_file = if self.flipped { 7 - file } else { file };
                    let display_rank = if self.flipped { 7 - rank } else { rank };
                    let x = self.area.x + (display_file as i16) * SQUARE_SIZE as i16;
                    let y = self.area.y + ((7 - display_rank as i16)) * SQUARE_SIZE as i16;
                    renderer.draw_rectangle(
                        Rectangle::new(x, y, SQUARE_SIZE, SQUARE_SIZE),
                        bg,
                        true,
                    )?;
                }
            }

            if let Some(board) = self.position.clone() {
                for sq in 0..64u8 {
                    if let Some(piece) = board.piece_at(sq) {
                        self.draw_piece(renderer, sq, piece)?;
                    }
                }
            }

            renderer.draw_rectangle(self.area.clone(), DrawColor::Black, false)?;
            // After a full paint there is nothing on screen, so any prior
            // highlight is gone too.
            self.last_drawn_selection = None;
            self.last_drawn_last_move = 0;
        }

        // Last-move diff: clear squares whose bracket is going away (scrub —
        // thick borders ghost on e-ink), skip them if the position diff
        // already repainted them. New brackets are stamped after the
        // selection diff so the layering is deterministic.
        let prev_lm = self.last_drawn_last_move;
        let curr_lm = self.last_move_squares;
        let removed_lm = prev_lm & !curr_lm;
        if removed_lm != 0 {
            for sq in 0..64u8 {
                if removed_lm & (1u64 << sq) == 0 || repainted & (1u64 << sq) != 0 {
                    continue;
                }
                let new_piece = self.position.as_ref().and_then(|b| b.piece_at(sq));
                self.repaint_square(renderer, sq, new_piece, true)?;
                repainted |= 1u64 << sq;
            }
        }

        // Selection diff: cheap because at most two squares are involved (old
        // highlight off, new highlight on). Squares that an earlier diff
        // already repainted are clean, so we don't need to scrub them again
        // before drawing the highlight.
        let prev_sel = self.last_drawn_selection;
        let curr_sel = self.selected_square;
        if prev_sel != curr_sel {
            if let Some(old) = prev_sel {
                let old_idx = square_to_index(old);
                if repainted & (1u64 << old_idx) == 0 {
                    let new_piece = self
                        .position
                        .as_ref()
                        .and_then(|b| b.piece_at(old_idx));
                    // Selection clear repaints with scrub — diagonal cuts
                    // ghost the same way a thick border does.
                    self.repaint_square(renderer, old_idx, new_piece, true)?;
                    repainted |= 1u64 << old_idx;
                }
            }
        }

        // Re-stamp last-move brackets on every currently-active last-move
        // square that needs it: newly added (not in prev) OR an earlier diff
        // repainted the square (which cleared whatever was on it).
        let stamp_lm = (curr_lm & !prev_lm) | (curr_lm & repainted);
        if stamp_lm != 0 {
            for sq in 0..64u8 {
                if stamp_lm & (1u64 << sq) != 0 {
                    self.draw_last_move_highlight(renderer, sq)?;
                }
            }
        }

        // Selection on top of the last-move bracket — they share corner
        // territory and the diagonal cut reads more cleanly drawn last.
        if let Some(sel) = curr_sel {
            let sel_idx = square_to_index(sel);
            // Stamp when it's a new selection OR when an earlier diff
            // repainted the selected square (clearing the prior cut).
            if Some(sel) != prev_sel || repainted & (1u64 << sel_idx) != 0 {
                self.draw_selection_highlight(renderer, sel)?;
            }
        }

        self.last_drawn_position = self.position.clone();
        self.last_drawn_selection = self.selected_square;
        self.last_drawn_last_move = self.last_move_squares;
        self.force_full_repaint = false;
        Ok(())
    }

    /// Repaint a single square: optional white-scrub for ghost mitigation,
    /// then the target color, then the new piece (if any). `scrub` should be
    /// true whenever the square previously held bright content (a piece or a
    /// selection's black border) — the white pass forces the e-ink panel
    /// through a full waveform cycle so the previous content doesn't bleed
    /// through the final color.
    fn repaint_square(
        &self,
        renderer: &mut Renderer,
        sq: u8,
        new_piece: Option<(Color, Piece)>,
        scrub: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let file = sq % 8;
        let rank = sq / 8;
        let is_dark = (file + rank) % 2 == 0;
        let bg = if is_dark {
            DrawColor::DarkGray
        } else {
            DrawColor::LightGray
        };

        let display_file = if self.flipped { 7 - file } else { file };
        let display_rank = if self.flipped { 7 - rank } else { rank };
        let x = self.area.x + (display_file as i16) * SQUARE_SIZE as i16;
        let y = self.area.y + ((7 - display_rank as i16)) * SQUARE_SIZE as i16;
        let rect = Rectangle::new(x, y, SQUARE_SIZE, SQUARE_SIZE);

        if scrub {
            // White intermediate flushes residual imprints — the panel runs a
            // bright→dark waveform on the second fill instead of trying to
            // converge from "mostly the previous content" to the target.
            renderer.draw_rectangle(rect, DrawColor::White, true)?;
        }
        renderer.draw_rectangle(rect, bg, true)?;

        if let Some(piece) = new_piece {
            self.draw_piece(renderer, sq, piece)?;
        }
        Ok(())
    }

    /// Draw a piece sprite over its square, alpha-composited against the
    /// square's intrinsic color. Caller is responsible for having already
    /// painted the square background.
    fn draw_piece(
        &self,
        renderer: &mut Renderer,
        sq: u8,
        piece: (Color, Piece),
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(sprite) = self.piece_sprites.get(piece.0, piece.1) else {
            return Ok(());
        };
        let file = sq % 8;
        let rank = sq / 8;
        let bg = if (file + rank) % 2 == 0 {
            DrawColor::DarkGray
        } else {
            DrawColor::LightGray
        };
        let display_file = if self.flipped { 7 - file } else { file };
        let display_rank = if self.flipped { 7 - rank } else { rank };
        let x = self.area.x
            + (display_file as i16) * SQUARE_SIZE as i16
            + ((SQUARE_SIZE - PIECE_DRAW_SIZE) / 2) as i16;
        let y = self.area.y
            + ((7 - display_rank as i16)) * SQUARE_SIZE as i16
            + ((SQUARE_SIZE - PIECE_DRAW_SIZE) / 2) as i16;
        renderer.draw_image_alpha(x, y, PIECE_DRAW_SIZE, PIECE_DRAW_SIZE, sprite, bg)?;
        Ok(())
    }

    /// Encode `mv` as a UCI string for the Lichess board API. For a pawn that
    /// lands on its last rank, append a queen promotion suffix — there is no
    /// promotion-picker UI yet, and queen is the practical default. Returns
    /// `None` if no position is loaded yet (shouldn't happen — moves are only
    /// emitted after `set_position`).
    pub fn move_to_uci(&self, mv: ChessMove) -> Option<String> {
        let from_idx = mv.from.rank * 8 + mv.from.file;
        let last_rank = match self.position.as_ref()?.piece_at(from_idx) {
            Some((Color::White, Piece::Pawn)) => Some(7u8),
            Some((Color::Black, Piece::Pawn)) => Some(0u8),
            _ => None,
        };
        let mut uci = format!("{}{}", mv.from.to_algebraic(), mv.to.to_algebraic());
        if last_rank == Some(mv.to.rank) {
            uci.push('q');
        }
        Some(uci)
    }

    /// Draw the selection highlight: four 45° lines starting at each corner
    /// and pointing toward the centre, each running `SEGMENT_LEN` along both
    /// axes — so each line's length is a quarter of the square's diagonal.
    /// Drawing individual lines (rather than a full border) keeps the
    /// centre of the square clear so the piece glyph stays readable.
    fn draw_selection_highlight(
        &self,
        renderer: &mut Renderer,
        sq: Square,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let file = if self.flipped { 7 - sq.file } else { sq.file };
        let rank = if self.flipped { 7 - sq.rank } else { sq.rank };
        let x = self.area.x + (file as i16) * SQUARE_SIZE as i16;
        let y = self.area.y + ((7 - rank as i16)) * SQUARE_SIZE as i16;
        let s = SQUARE_SIZE as i16;
        let l = SEGMENT_LEN;
        let color = highlight_color(sq.file, sq.rank);

        // (x1, y1) sits on the corner, (x2, y2) is `l` along both axes
        // toward the square's centre.
        let cuts = [
            // Top-left → centre
            (x, y, x + l, y + l),
            // Top-right → centre
            (x + s, y, x + s - l, y + l),
            // Bottom-right → centre
            (x + s, y + s, x + s - l, y + s - l),
            // Bottom-left → centre
            (x, y + s, x + l, y + s - l),
        ];
        for (x1, y1, x2, y2) in cuts {
            renderer.draw_line(x1, y1, x2, y2, color, SELECTION_STROKE)?;
        }
        Ok(())
    }

    /// Draw the last-move highlight: each corner gets an "L" of two
    /// `SEGMENT_LEN`-long, `LAST_MOVE_STROKE`-thick filled rectangles
    /// running along the adjacent edges. Filled rects (not poly_segment)
    /// because the segments are axis-aligned and we want sharp corners
    /// without GC line-width juggling.
    fn draw_last_move_highlight(
        &self,
        renderer: &mut Renderer,
        sq: u8,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let file = sq % 8;
        let rank = sq / 8;
        let display_file = if self.flipped { 7 - file } else { file };
        let display_rank = if self.flipped { 7 - rank } else { rank };
        let x = self.area.x + (display_file as i16) * SQUARE_SIZE as i16;
        let y = self.area.y + ((7 - display_rank as i16)) * SQUARE_SIZE as i16;
        let s = SQUARE_SIZE as i16;
        let l = SEGMENT_LEN;
        let t = LAST_MOVE_STROKE;
        let color = highlight_color(file, rank);

        // For each corner (cx, cy) the two arms run inward along the adjacent
        // edges; horizontal arms are L×t, vertical arms are t×L. The corner
        // cell (t×t) is covered twice by overlap — fine, both rects fill the
        // same colour.
        let arms: [(i16, i16, u16, u16); 8] = [
            // Top-left  ── horizontal & vertical
            (x, y, l as u16, t as u16),
            (x, y, t as u16, l as u16),
            // Top-right
            (x + s - l, y, l as u16, t as u16),
            (x + s - t, y, t as u16, l as u16),
            // Bottom-right
            (x + s - l, y + s - t, l as u16, t as u16),
            (x + s - t, y + s - l, t as u16, l as u16),
            // Bottom-left
            (x, y + s - t, l as u16, t as u16),
            (x, y + s - l, t as u16, l as u16),
        ];
        for (rx, ry, rw, rh) in arms {
            renderer.draw_rectangle(Rectangle::new(rx, ry, rw, rh), color, true)?;
        }
        Ok(())
    }
}

// Pick a contrasting highlight colour for square (`file`, `rank`): dark
// squares get the light-square colour so the corner decorations read against
// the background, light squares stay black.
fn highlight_color(file: u8, rank: u8) -> DrawColor {
    if (file + rank) % 2 == 0 {
        DrawColor::LightGray
    } else {
        DrawColor::Black
    }
}

fn square_to_index(sq: Square) -> u8 {
    sq.rank * 8 + sq.file
}
