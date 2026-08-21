mod gamestate;
mod graphics;
mod utils;

use std::io::Write;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;

use chess::Color as ChessColor;
use chess::*;
use chessian::chooser::*;
use chessian::timecontrol::*;
use chessian::*;
use macroquad::color::Color;
use macroquad::input::KeyCode;
use macroquad::prelude::*;
use macroquad::ui::*;

use gamestate::GameState;
use graphics::Textures;
use utils::board_to_fen;

/// Size (in pixels) of the chess squares
pub const FIELD_SIZE: f32 = 100.0;
/// The color used for light squares
pub const COLOR_WHITE: Color = Color::from_hex(0xFFFFF2);
/// The color used for dark squares
pub const COLOR_BLACK: Color = Color::from_hex(0xFFC0CB);
/// A blue color used for accents
pub const COLOR_BLUE: Color = Color::from_hex(0xB3EBF2);
/// A red color used for accents
pub const COLOR_RED: Color = Color::from_hex(0x8fbc8f);
/// The radius (in pixels) of the circles indicating legal moves
pub const MOVE_INDICATOR_SIZE: f32 = 15.0;
/// The color of the move indicator circle
pub const MOVE_INDICATOR_COLOR: Color = Color::new(1., 0.1, 0.1, 0.5);

/// The width (in pixels) of the evaluation bar
pub const EVAL_BAR_W: f32 = 35.0;

/// The width (in pixels) of the side bar gui
pub const UI_WIDTH: f32 = 200.0;
const UI_ID_CHECKBOX: Id = 0;
const UI_ID_CHECKBOX_DSN: Id = 2;
const UI_ID_CHECKBOX_DP: Id = 3;
const UI_ID_SLIDER: Id = 4;
const UI_ID_EVAL: Id = 666;

/// State of the chess gui.
#[derive(Debug)]
struct GuiState {
    /// The alpha rating (in centipawns) of the last move by the computer.
    last_alpha: Option<i32>,
    /// The depth the computer reached during its last search, sans q-search.
    last_depth: Option<usize>,
    /// The amount of milliseconds the computer last searched for in total.
    last_millis: Option<u128>,
    /// Automatically move after the play moved?
    auto_respond: bool,
    /// Should the engine make a move next frame?
    engine_move_next_frame: bool,
    /// Draw square names?
    draw_square_names: bool,
    /// Draw pieces?
    draw_pieces: bool,
    /// How long the computer should search in total.
    thinking_millis: u128,
    /// Invert the board?
    invert: bool,
    /// Evaluate the position in the background?
    bg_eval: bool,
    /// The current depth of the background evaluation.
    bg_eval_depth: usize,
    /// The current best move of the background evaluation.
    bg_eval_best_move: Option<ChessMove>,
    /// The stop flag of the background evaluation.
    bg_eval_stop_flag: Arc<AtomicBool>,
    /// The handle to the background evaluation thread.
    bg_eval_handle: mpsc::Receiver<Option<ChooserResult>>,
    /// The square of the piece being dragged
    dragged_piece_square: Option<Square>,
}

#[macroquad::main(conf)]
async fn main() -> Result<(), String> {
    let mut args = std::env::args();
    let mut game_state = if let Some(fen) = args.nth(1) {
        GameState::from_fen(&fen)?
    } else {
        GameState::default()
    };

    let mut gui_state = GuiState::new(game_state.board());
    let piece_sprites = Textures::load("pieces.png", 16.0).await;
    let mut clickable_moves: Vec<ChessMove> = Vec::new();
    let mut pending_promotion_move: Option<ChessMove> = None;

    loop {
        let hovered_square = hovered_square(gui_state.invert);
        let is_mouse_in_board = mouse_position().0 <= FIELD_SIZE * 8.0;

        draw(
            &mut gui_state,
            &mut game_state,
            &piece_sprites,
            hovered_square,
            is_mouse_in_board,
        );
        try_recv_bg_eval(&mut gui_state, &mut game_state);

        if let Some(pending_promotion) = pending_promotion_move {
            promotion_menu(
                &mut gui_state,
                &mut game_state,
                &piece_sprites,
                pending_promotion,
                hovered_square,
            );
            pending_promotion_move = None;
        }

        if gui_state.engine_move_next_frame {
            engine_move(&mut gui_state, &mut game_state).await;
            clickable_moves.clear();
            continue;
        }

        if let Some(c) = get_char_pressed() {
            handle_char_pressed(&mut gui_state, &mut game_state, c, &mut clickable_moves);
        }

        if !is_mouse_in_board {
            next_frame().await;
            continue;
        }

        draw_clickable_moves(&gui_state, &clickable_moves);

        if is_mouse_button_pressed(MouseButton::Left) {
            handle_left_click(
                &mut gui_state,
                &mut game_state,
                hovered_square,
                &mut pending_promotion_move,
                &mut clickable_moves,
            );
        }

        if is_mouse_button_released(MouseButton::Left) && gui_state.dragged_piece_square.is_some() {
            handle_left_click(
                &mut gui_state,
                &mut game_state,
                hovered_square,
                &mut pending_promotion_move,
                &mut clickable_moves,
            );
            gui_state.dragged_piece_square = None;
        }

        next_frame().await
    }
}

fn draw(
    gui_state: &mut GuiState,
    game_state: &mut GameState,
    piece_sprites: &Textures,
    hovered_square: Square,
    is_mouse_in_board: bool,
) {
    draw_ui(gui_state, game_state);
    draw_eval_bar(gui_state);
    draw_board(
        gui_state,
        game_state,
        piece_sprites,
        hovered_square,
        is_mouse_in_board,
    );
    draw_bg_eval_best_move(gui_state);
}

fn draw_text_centered(text: &str, font_size: f32, color: Color) {
    let screen_w = screen_width();
    let screen_h = screen_height();

    let dims = measure_text(text, None, font_size as u16, 1.0);
    let x = (screen_w - dims.width) / 2.0;
    let y = (screen_h + dims.height) / 2.0;

    draw_text(text, x, y, font_size, color);
}

fn invert_square(sq: Square) -> Square {
    Square::make_square(
        Rank::from_index(7 - sq.get_rank().to_index()),
        File::from_index(7 - sq.get_file().to_index()),
    )
}

fn square_to_xy(square: Square) -> (f32, f32) {
    (
        square.get_file().to_index() as f32 * FIELD_SIZE,
        (7 - square.get_rank().to_index()) as f32 * FIELD_SIZE,
    )
}

fn hovered_piece(board: &Board, invert: bool) -> Option<(Piece, ChessColor)> {
    let square = hovered_square(invert);
    board.piece_on(square).zip(board.color_on(square))
}

fn hovered_square(invert: bool) -> Square {
    let (x, y) = mouse_position();
    let sq = square_under(x, y);
    if invert { invert_square(sq) } else { sq }
}

fn square_under(x: f32, y: f32) -> Square {
    Square::make_square(
        Rank::from_index(7 - (y / FIELD_SIZE) as usize),
        File::from_index((x / FIELD_SIZE) as usize),
    )
}

fn draw_piece(piece: Piece, color: ChessColor, x: f32, y: f32, piece_sprites: &Textures) {
    let texture = &Texture2D::from_image(piece_sprites.get_piece((piece, color)));
    texture.set_filter(FilterMode::Nearest);
    draw_texture_ex(
        texture,
        x,
        y,
        WHITE,
        DrawTextureParams {
            dest_size: Some(vec2(FIELD_SIZE, FIELD_SIZE)),
            ..Default::default()
        },
    );
}

fn spawn_new_eval_thread(
    board: HistoryBoard,
    stop_flag: &mut Arc<AtomicBool>,
    eval_depth: usize,
    rec: &mut mpsc::Receiver<Option<ChooserResult>>,
) {
    stop_flag.store(true, Ordering::Relaxed);
    // wait for old eval thread to stop
    let _ = rec.recv();
    *stop_flag = Arc::new(AtomicBool::new(false));
    *rec = spawn_eval_thread(board, eval_depth, stop_flag.clone());
}

fn spawn_eval_thread(
    board: HistoryBoard,
    depth: usize,
    stop_flag: Arc<AtomicBool>,
) -> mpsc::Receiver<Option<ChooserResult>> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let eval = best_move(
            &board,
            TimeControl::new(Some(stop_flag), TCMode::Depth(depth)),
            std::io::sink(),
            std::io::sink(),
        );
        tx.send(eval)
    });

    rx
}

fn draw_ui(gui_state: &mut GuiState, game_state: &mut GameState) {
    root_ui().window(
        hash!(),
        Vec2::new(FIELD_SIZE * 8.0 + EVAL_BAR_W, 0.0),
        Vec2::new(UI_WIDTH, FIELD_SIZE * 8.0),
        |ui| {
            ui.separator();
            if let Some(alpha) = gui_state.last_alpha {
                ui.label(None, &format!("Eval: {}", alpha));
            } else {
                ui.label(None, "Eval: None");
            }
            if gui_state.bg_eval {
                ui.label(None, &format!("Eval depth: {}", gui_state.bg_eval_depth));
            } else {
                ui.label(None, "No eval");
            }
            let prev_eval = gui_state.bg_eval;
            ui.checkbox(UI_ID_EVAL, "Eval", &mut gui_state.bg_eval);
            if !gui_state.bg_eval {
                gui_state.bg_eval_stop_flag.store(true, Ordering::Relaxed);
            } else if !prev_eval {
                gui_state.bg_eval_depth = 1;
                spawn_new_eval_thread(
                    game_state.board().clone(),
                    &mut gui_state.bg_eval_stop_flag,
                    gui_state.bg_eval_depth,
                    &mut gui_state.bg_eval_handle,
                );
            }
            if let Some(depth) = gui_state.last_depth {
                ui.label(None, &format!("Last depth: {}", depth));
            } else {
                ui.label(None, "Last depth: None");
            }
            if let Some(millis) = gui_state.last_millis {
                ui.label(
                    None,
                    &format!("Last search: {:.3}s", millis as f64 / 1_000.0),
                );
            } else {
                ui.label(None, "Last search: None");
            }
            ui.separator();
            ui.checkbox(UI_ID_CHECKBOX, "Auto respond", &mut gui_state.auto_respond);
            ui.checkbox(
                UI_ID_CHECKBOX_DSN,
                "Square names",
                &mut gui_state.draw_square_names,
            );
            ui.checkbox(UI_ID_CHECKBOX_DP, "Draw pieces", &mut gui_state.draw_pieces);
            ui.label(None, &format!("Game: {:?}", game_state.board().status()));
            let mut seconds = gui_state.thinking_millis as f32 / 1000.0;
            ui.slider(UI_ID_SLIDER, "Search time", 0.5..120.0, &mut seconds);
            if ui.button(None, "1s") {
                seconds = 1.0;
            }
            if ui.button(None, "3s") {
                seconds = 3.0;
            }
            if ui.button(None, "5s") {
                seconds = 5.0;
            }
            if ui.button(None, "10s") {
                seconds = 10.0;
            }
            gui_state.thinking_millis = (seconds * 1000.0) as u128;
            if ui.button(None, "GO, GO, GO!") {
                gui_state.engine_move_next_frame = true;
            }
            if ui.button(None, "< undo") {
                game_state.undo_move();
                if gui_state.bg_eval {
                    gui_state.bg_eval_depth = 1;
                    spawn_new_eval_thread(
                        game_state.board().clone(),
                        &mut gui_state.bg_eval_stop_flag,
                        gui_state.bg_eval_depth,
                        &mut gui_state.bg_eval_handle,
                    );
                }
            }
            ui.same_line(50.0);
            if ui.button(None, "redo >") {
                game_state.redo_move();
                if gui_state.bg_eval {
                    gui_state.bg_eval_depth = 1;
                    spawn_new_eval_thread(
                        game_state.board().clone(),
                        &mut gui_state.bg_eval_stop_flag,
                        gui_state.bg_eval_depth,
                        &mut gui_state.bg_eval_handle,
                    );
                }
            }
        },
    );
}

fn draw_board(
    gui_state: &GuiState,
    game_state: &GameState,
    piece_sprites: &Textures,
    hovered_square: Square,
    is_mouse_in_board: bool,
) {
    let mut draw_dragged_piece = None;
    for y in 0..=7 {
        for x in 0..=7 {
            let square = Square::make_square(
                Rank::from_index(if gui_state.invert { y } else { 7 - y }),
                File::from_index(if gui_state.invert { 7 - x } else { x }),
            );
            let x_pos = x as f32 * FIELD_SIZE;
            let y_pos = y as f32 * FIELD_SIZE;
            let (color, opp_color) = if (x + y) % 2 == 0 {
                (COLOR_WHITE, COLOR_BLACK)
            } else {
                (COLOR_BLACK, COLOR_WHITE)
            };
            // Draw field
            draw_rectangle(x_pos, y_pos, FIELD_SIZE, FIELD_SIZE, color);
            if square == hovered_square && is_mouse_in_board {
                draw_rectangle_lines(x_pos, y_pos, FIELD_SIZE, FIELD_SIZE, 7.5, COLOR_BLUE);
            }
            if gui_state.dragged_piece_square == Some(square) {
                draw_rectangle_lines(x_pos, y_pos, FIELD_SIZE, FIELD_SIZE, 7.5, COLOR_RED);
            }
            // Draw piece?
            if gui_state.draw_pieces
                && let Some((piece, color)) = game_state
                    .board()
                    .piece_on(square)
                    .zip(game_state.board().color_on(square))
            {
                if gui_state.dragged_piece_square == Some(square) {
                    draw_dragged_piece = Some(move || {
                        draw_piece(
                            piece,
                            color,
                            mouse_position().0 - FIELD_SIZE / 2.0,
                            mouse_position().1 - FIELD_SIZE / 2.0,
                            piece_sprites,
                        );
                    });
                } else {
                    draw_piece(piece, color, x_pos, y_pos, piece_sprites);
                }
            }

            if gui_state.draw_square_names {
                draw_text(
                    &square.to_string(),
                    x_pos,
                    y_pos + FIELD_SIZE,
                    20.0,
                    opp_color,
                );
            }

            if let Some(m) = game_state.last_move()
                && (m.get_source() == square || m.get_dest() == square)
            {
                draw_rectangle_lines(x_pos, y_pos, FIELD_SIZE, FIELD_SIZE, 7.5, COLOR_RED);
            }
        }
    }
    if let Some(draw_dragged_piece) = draw_dragged_piece {
        draw_dragged_piece();
    }
}

fn draw_bg_eval_best_move(gui_state: &GuiState) {
    if let Some(r) = gui_state.bg_eval_best_move
        && gui_state.bg_eval
    {
        let (x0, y0) = square_to_xy(if gui_state.invert {
            invert_square(r.get_source())
        } else {
            r.get_source()
        });
        let (x1, y1) = square_to_xy(if gui_state.invert {
            invert_square(r.get_dest())
        } else {
            r.get_dest()
        });
        draw_line(
            x0 + FIELD_SIZE / 2.0,
            y0 + FIELD_SIZE / 2.0,
            x1 + FIELD_SIZE / 2.0,
            y1 + FIELD_SIZE / 2.0,
            5.0,
            COLOR_RED,
        );
    }
}

fn promotion_menu(
    gui_state: &mut GuiState,
    game_state: &mut GameState,
    piece_sprites: &Textures,
    pawn_move: ChessMove,
    hovered_square: Square,
) {
    let dest = pawn_move.get_dest();
    let to_inner_board = if dest.get_rank() == Rank::First {
        Square::up
    } else {
        Square::down
    };
    let queen_sq = to_inner_board(&dest).unwrap();
    let rook_sq = to_inner_board(&queen_sq).unwrap();
    let bishop_sq = to_inner_board(&rook_sq).unwrap();
    let knight_sq = to_inner_board(&bishop_sq).unwrap();
    for (square, piece) in [queen_sq, rook_sq, bishop_sq, knight_sq]
        .iter()
        .zip([Piece::Queen, Piece::Rook, Piece::Bishop, Piece::Knight].iter())
    {
        let (x, y) = square_to_xy(if gui_state.invert {
            invert_square(*square)
        } else {
            *square
        });
        draw_piece(
            *piece,
            game_state.board().side_to_move(),
            x,
            y,
            piece_sprites,
        );
    }
    if is_mouse_button_pressed(MouseButton::Left) {
        let clicked_promotion = if hovered_square == queen_sq {
            Some(Piece::Queen)
        } else if hovered_square == rook_sq {
            Some(Piece::Rook)
        } else if hovered_square == bishop_sq {
            Some(Piece::Bishop)
        } else if hovered_square == knight_sq {
            Some(Piece::Knight)
        } else {
            None
        };
        if let Some(promotion) = clicked_promotion {
            game_state.make_move(ChessMove::new(
                pawn_move.get_source(),
                dest,
                Some(promotion),
            ));
            if gui_state.bg_eval {
                gui_state.bg_eval_depth = 1;
                spawn_new_eval_thread(
                    game_state.board().clone(),
                    &mut gui_state.bg_eval_stop_flag,
                    gui_state.bg_eval_depth,
                    &mut gui_state.bg_eval_handle,
                );
            }
        }
    }
}

fn draw_eval_bar(gui_state: &GuiState) {
    if let Some(score) = gui_state.last_alpha {
        let pawn_score = score as f32 / 100.0;
        let bar_y = FIELD_SIZE * 4.0 + pawn_score * 25.0;
        draw_rectangle(FIELD_SIZE * 8.0, bar_y, EVAL_BAR_W, FIELD_SIZE * 8.0, BLACK);
        draw_rectangle(FIELD_SIZE * 8.0, 0.0, EVAL_BAR_W, bar_y, COLOR_WHITE);
        draw_text(
            &format!("{pawn_score:.1}"),
            FIELD_SIZE * 8.0,
            FIELD_SIZE * 4.0,
            15.0,
            COLOR_RED,
        );
    } else {
        draw_rectangle(FIELD_SIZE * 8.0, 0.0, EVAL_BAR_W, FIELD_SIZE * 8.0, GRAY);
    }
}

fn try_recv_bg_eval(gui_state: &mut GuiState, game_state: &mut GameState) {
    if let Ok(Some(result)) = gui_state.bg_eval_handle.try_recv() {
        gui_state.last_alpha = Some(if game_state.board().side_to_move() == ChessColor::Black {
            -result.deep_eval
        } else {
            result.deep_eval
        });
        gui_state.bg_eval_best_move = Some(result.best_move);
        if gui_state.bg_eval {
            gui_state.bg_eval_depth += 1;
            spawn_new_eval_thread(
                game_state.board().clone(),
                &mut gui_state.bg_eval_stop_flag,
                gui_state.bg_eval_depth,
                &mut gui_state.bg_eval_handle,
            );
        }
    }
}

fn restart_bg_eval(gui_state: &mut GuiState, game_state: &GameState) {
    gui_state.bg_eval_depth = 1;
    spawn_new_eval_thread(
        game_state.board().clone(),
        &mut gui_state.bg_eval_stop_flag,
        gui_state.bg_eval_depth,
        &mut gui_state.bg_eval_handle,
    );
}

async fn engine_move(gui_state: &mut GuiState, game_state: &mut GameState) {
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.0, 0.0, 0.0, 0.75),
    );
    draw_text_centered("Engine calculates ...", 35.0, COLOR_BLUE);
    next_frame().await;
    if let Some(result) = game_state.engine_move(TimeControl::new(
        None,
        TCMode::MoveTime(gui_state.thinking_millis),
    )) {
        gui_state.last_alpha = Some(result.deep_eval);
        gui_state.last_depth = Some(result.reached_depth);
        gui_state.last_millis = Some(result.millis);
    }
    gui_state.engine_move_next_frame = false;
    if gui_state.bg_eval {
        restart_bg_eval(gui_state, game_state);
    }
}

fn draw_clickable_moves(gui_state: &GuiState, clickable_moves: &[ChessMove]) {
    for m in clickable_moves {
        let dest = m.get_dest();
        let (x, y) = square_to_xy(if gui_state.invert {
            invert_square(dest)
        } else {
            dest
        });
        draw_circle(
            x + FIELD_SIZE / 2.,
            y + FIELD_SIZE / 2.,
            MOVE_INDICATOR_SIZE,
            MOVE_INDICATOR_COLOR,
        );
    }
}

fn handle_left_click(
    gui_state: &mut GuiState,
    game_state: &mut GameState,
    hovered_square: Square,
    pending_promotion_move: &mut Option<ChessMove>,
    clickable_moves: &mut Vec<ChessMove>,
) {
    let side_to_move_clicked = hovered_piece(game_state.board(), gui_state.invert)
        .map(|(_, color)| color == game_state.board().side_to_move())
        .unwrap_or(false);
    if side_to_move_clicked {
        *clickable_moves = game_state.legal_moves_from(hovered_square);
        if gui_state.dragged_piece_square.is_none() {
            gui_state.dragged_piece_square = Some(hovered_square);
        }
    } else {
        if let Some(m) = clickable_moves
            .iter()
            .find(|m| m.get_dest() == hovered_square)
        {
            let mov = *m;
            if mov.get_promotion().is_some() {
                *pending_promotion_move = Some(mov);
            } else {
                game_state.make_move(mov);
                if gui_state.bg_eval {
                    restart_bg_eval(gui_state, game_state);
                }
                gui_state.engine_move_next_frame = gui_state.auto_respond;
            }
        }
        clickable_moves.clear();
    }
}

fn handle_char_pressed(
    gui_state: &mut GuiState,
    game_state: &mut GameState,
    c: char,
    clickable_moves: &mut Vec<ChessMove>,
) {
    let control_down = if cfg!(target_os = "macos") {
        is_key_down(KeyCode::LeftSuper) || is_key_down(KeyCode::RightSuper)
    } else {
        is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl)
    };
    match c {
        'a' => gui_state.auto_respond = !gui_state.auto_respond,
        'f' => println!("{}", board_to_fen(game_state.board())),
        'm' => {
            gui_state.engine_move_next_frame = true;
            clickable_moves.clear();
        }
        'z' if control_down => {
            if game_state.undo_move() {
                clickable_moves.clear();
                if gui_state.bg_eval {
                    restart_bg_eval(gui_state, game_state);
                }
            }
        }
        'y' if control_down => {
            if game_state.redo_move() {
                clickable_moves.clear();
                if gui_state.bg_eval {
                    restart_bg_eval(gui_state, game_state);
                }
            }
        }
        's' => gui_state.draw_square_names = !gui_state.draw_square_names,
        'p' => gui_state.draw_pieces = !gui_state.draw_pieces,
        'i' => gui_state.invert = !gui_state.invert,
        'r' => *game_state = GameState::default(),
        't' => {
            let history = game_state.history();
            println!("Analyzing game. Will take {} seconds", history.len() * 3);
            for (b, _) in history {
                let result = best_move(
                    b,
                    TimeControl::new(None, TCMode::MoveTime(3000)),
                    std::io::sink(),
                    std::io::sink(),
                )
                .unwrap();
                print!("{}", result.deep_eval);
                let _ = std::io::stdout().flush();
            }
        }
        _otherwise => (),
    }
}

impl GuiState {
    fn new(board: &HistoryBoard) -> Self {
        let bg_eval_stop_flag = Arc::new(AtomicBool::new(false));
        Self {
            last_alpha: None,
            last_depth: None,
            last_millis: None,
            auto_respond: true,
            engine_move_next_frame: false,
            draw_square_names: true,
            draw_pieces: true,
            thinking_millis: 3_000,
            invert: false,
            bg_eval: true,
            bg_eval_depth: 1,
            bg_eval_best_move: None,
            bg_eval_stop_flag: bg_eval_stop_flag.clone(),
            bg_eval_handle: spawn_eval_thread(board.clone(), 1, bg_eval_stop_flag.clone()),
            dragged_piece_square: None,
        }
    }
}

fn conf() -> Conf {
    Conf {
        window_title: "Chessian".to_owned(),
        window_width: 8 * FIELD_SIZE as i32 + EVAL_BAR_W as i32 + UI_WIDTH as i32,
        window_height: 8 * FIELD_SIZE as i32,
        window_resizable: false,
        ..Default::default()
    }
}
