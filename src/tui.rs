use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::{execute, terminal};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Borders, Chart, Clear, Dataset, GraphType, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{
    App, ControlAnimationKind, ControlHover, HistoryActionHover, LaunchRequest, Overlay,
    SettingsButtonHover, UiPulseKind, dictionary_languages, dictionary_names,
    heatmap_language_label_for_config, interface_language_label_for_config, is_russian_interface,
    numeric_setting, on_off_for_config, raw_speed_value, speed_unit, speed_value,
};
use crate::audio::KeyClickPlayer;
use crate::config::Config;
use crate::keyboard::{
    HEATMAP_EN_SHIFT_SYMBOL_ROW_1, HEATMAP_EN_SHIFT_SYMBOL_ROW_2, HEATMAP_RU_EXTRA_SYMBOL_ROW_1,
    HEATMAP_RU_EXTRA_SYMBOL_ROW_2,
};
use crate::storage::ResultRow;
use crate::typing::{CharState, Mode, QuoteLength, SpeedSample, TestState, mode_label};

const TEXT_WIDTH: u16 = 116;
const CONTROLS_WIDTH: u16 = 130;
const SETTINGS_WIDTH: u16 = 82;
const SETTINGS_HEIGHT: u16 = 30;
const SETTINGS_LABEL_WIDTH: usize = 17;
const SETTINGS_CONTROL_WIDTH: usize = 20;
const SETTINGS_HINT_WIDTH: usize = 27;
const SETTINGS_VALUE_WIDTH: usize = 14;
const FIXED_UI_SCALE: u16 = 80;
const BORDER_ANIMATION_MS: f64 = 300.0;
const HEATMAP_INDEXED_PALETTE: [u8; 12] = [230, 226, 220, 214, 208, 202, 196, 160, 124, 88, 89, 90];
const HEATMAP_RGB_PALETTE: [Color; 6] = [
    Color::Rgb(255, 245, 157),
    Color::Rgb(255, 214, 10),
    Color::Rgb(255, 149, 0),
    Color::Rgb(255, 69, 0),
    Color::Rgb(220, 20, 60),
    Color::Rgb(128, 0, 64),
];
const HISTORY_TABLE_WIDTH: u16 = 74;

const SETTING_THEME: usize = 0;
const SETTING_VISUAL_STYLE: usize = 1;
const SETTING_CURSOR_STYLE: usize = 2;
const SETTING_INTERFACE_LANGUAGE: usize = 3;
const SETTING_LANGUAGE: usize = 4;
const SETTING_PUNCTUATION: usize = 5;
const SETTING_NUMBERS: usize = 6;
const SETTING_DIFFICULTY: usize = 7;
const SETTING_QUICK_RESTART: usize = 8;
const SETTING_REPEAT_QUOTES: usize = 9;
const SETTING_BLIND_MODE: usize = 10;
const SETTING_WORDS_HISTORY: usize = 11;
const SETTING_SPEED_UNIT: usize = 12;
const SETTING_MIN_SPEED: usize = 13;
const SETTING_MIN_ACCURACY: usize = 14;
const SETTING_MIN_WORD_BURST: usize = 15;
const SETTING_SAVE_RESULTS: usize = 16;
const SETTING_KEY_SOUND_STYLE: usize = 17;
const SETTING_LAST: usize = SETTING_KEY_SOUND_STYLE;

fn ui_ru(app: &App) -> bool {
    is_russian_interface(&app.config)
}

fn tr<'a>(app: &App, en: &'a str, ru: &'a str) -> &'a str {
    if ui_ru(app) { ru } else { en }
}

fn mode_label_ui(app: &App, mode: &Mode) -> String {
    if !ui_ru(app) {
        return mode_label(mode);
    }
    match mode {
        Mode::LastConfig => "время".to_string(),
        Mode::Time(seconds) => format!("время {seconds}"),
        Mode::Words(words) => format!("слова {words}"),
        Mode::Quote(length) => format!("цитата {}", quote_length_label_ui(app, *length)),
        Mode::Custom(_) => "свой текст".to_string(),
    }
}

fn quote_length_label_ui(app: &App, length: QuoteLength) -> &'static str {
    if !ui_ru(app) {
        return match length {
            QuoteLength::Short => "short",
            QuoteLength::Medium => "medium",
            QuoteLength::Long => "long",
            QuoteLength::Random => "random",
        };
    }
    match length {
        QuoteLength::Short => "короткая",
        QuoteLength::Medium => "средняя",
        QuoteLength::Long => "длинная",
        QuoteLength::Random => "случайная",
    }
}

pub fn run(config: Config, launch: LaunchRequest) -> Result<()> {
    let mut app = App::new(config, launch)?;
    execute!(io::stdout(), Hide)?;
    if app.config.mouse {
        execute!(io::stdout(), EnableMouseCapture)?;
    }

    let mut terminal = ratatui::init();
    let mut key_clicks = KeyClickPlayer::new(&app.config.key_sound_style);
    let result = run_loop(&mut terminal, &mut app, &mut key_clicks);
    ratatui::restore();

    let _ = execute!(io::stdout(), Show);
    if app.config.mouse {
        let _ = execute!(io::stdout(), DisableMouseCapture);
    }
    terminal::disable_raw_mode().ok();
    result
}

fn run_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    key_clicks: &mut KeyClickPlayer,
) -> Result<()> {
    loop {
        terminal.draw(|frame| render(frame, app))?;

        if app.should_quit {
            break;
        }

        if matches!(app.session.state, TestState::Running)
            && app
                .session
                .remaining()
                .is_some_and(|remaining| remaining.is_zero())
        {
            app.session.finish();
            app.save_finished_result();
            app.overlay = Overlay::Results;
            app.pulse(UiPulseKind::Overlay);
        }
        if app.session.state == TestState::Running {
            app.session.sample_speed();
        }
        app.enforce_fail_conditions();
        if app.session.state == TestState::Failed && app.overlay != Overlay::Results {
            app.save_finished_result();
            app.overlay = Overlay::Results;
            app.pulse(UiPulseKind::Overlay);
        }

        if event::poll(Duration::from_millis(32))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    handle_key(app, key, key_clicks)
                }
                Event::Resize(_, _) => {}
                Event::Mouse(mouse) => handle_mouse(app, mouse, key_clicks),
                _ => {}
            }
        }
    }
    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent, key_clicks: &mut KeyClickPlayer) {
    let previous_overlay = app.overlay;
    let previous_input_len = app.session.input.len();
    key_clicks.set_style(&app.config.key_sound_style);

    if key_binding_matches(key, &app.config.keybindings.quit) {
        app.should_quit = true;
        return;
    }

    if app.overlay == Overlay::None
        && is_safe_typing_screen_binding(app, key, &app.config.keybindings.restart)
    {
        app.restart_requested();
        app.pulse(UiPulseKind::Restart);
        return;
    }

    if app.overlay != Overlay::None {
        handle_overlay_key(app, key);
        pulse_after_interaction(app, previous_overlay, previous_input_len);
        return;
    }

    if handle_main_key(app, key) {
        pulse_after_interaction(app, previous_overlay, previous_input_len);
        return;
    }

    match key.code {
        KeyCode::Tab => {}
        KeyCode::Backspace => app.session.backspace(),
        KeyCode::Char(ch) => {
            app.session.type_char(ch);
            app.enforce_fail_conditions();
            if matches!(app.session.state, TestState::Finished | TestState::Failed) {
                app.save_finished_result();
                open_overlay(app, Overlay::Results);
            }
        }
        KeyCode::Enter => app.session.type_char('\n'),
        _ => {}
    }

    pulse_after_interaction(app, previous_overlay, previous_input_len);
    if app.session.input.len() != previous_input_len {
        key_clicks.play_key();
    }
}

fn handle_main_key(app: &mut App, key: KeyEvent) -> bool {
    if is_safe_typing_screen_binding(app, key, &app.config.keybindings.help) {
        open_overlay(app, Overlay::Help);
        return true;
    }
    if is_safe_typing_screen_binding(app, key, &app.config.keybindings.pause) {
        app.session.pause();
        open_overlay(app, Overlay::Pause);
        return true;
    }
    if is_safe_typing_screen_binding(app, key, &app.config.keybindings.retry_text) {
        app.retry_same_text();
        app.pulse(UiPulseKind::Restart);
        return true;
    }
    if is_safe_typing_screen_binding(app, key, &app.config.keybindings.save_result) {
        app.save_finished_result();
        return true;
    }
    if can_open_result_from_status(app) {
        if is_safe_typing_screen_binding(app, key, &app.config.keybindings.left)
            || is_safe_typing_screen_binding(app, key, &app.config.keybindings.right)
        {
            app.control_hover = Some(ControlHover::ResultOpen);
            app.animate_control(ControlHover::ResultOpen, ControlAnimationKind::HoverIn);
            return true;
        }
        if is_safe_typing_screen_binding(app, key, &app.config.keybindings.activate) {
            open_overlay(app, Overlay::Results);
            return true;
        }
    }
    if matches!(app.session.state, TestState::Running | TestState::Paused) {
        return false;
    }
    if is_safe_typing_screen_binding(app, key, &app.config.keybindings.toggle_punctuation) {
        let was_active = app.config.punctuation;
        app.control_hover = Some(ControlHover::Punctuation);
        app.toggle_punctuation();
        app.animate_control(
            ControlHover::Punctuation,
            if was_active {
                ControlAnimationKind::ActivateOut
            } else {
                ControlAnimationKind::ActivateIn
            },
        );
        app.pulse(UiPulseKind::Setting);
        return true;
    }
    if is_safe_typing_screen_binding(app, key, &app.config.keybindings.toggle_numbers) {
        let was_active = app.config.numbers;
        app.control_hover = Some(ControlHover::Numbers);
        app.toggle_numbers();
        app.animate_control(
            ControlHover::Numbers,
            if was_active {
                ControlAnimationKind::ActivateOut
            } else {
                ControlAnimationKind::ActivateIn
            },
        );
        app.pulse(UiPulseKind::Setting);
        return true;
    }
    if is_safe_typing_screen_binding(app, key, &app.config.keybindings.mode_time) {
        app.control_hover = Some(ControlHover::Time);
        app.set_mode(Mode::Time(app.config.default_time));
        app.animate_control(ControlHover::Time, ControlAnimationKind::ActivateIn);
        app.pulse(UiPulseKind::Setting);
        return true;
    }
    if is_safe_typing_screen_binding(app, key, &app.config.keybindings.mode_words) {
        app.control_hover = Some(ControlHover::Words);
        app.set_mode(Mode::Words(app.config.default_words));
        app.animate_control(ControlHover::Words, ControlAnimationKind::ActivateIn);
        app.pulse(UiPulseKind::Setting);
        return true;
    }
    if is_safe_typing_screen_binding(app, key, &app.config.keybindings.mode_quote) {
        app.control_hover = Some(ControlHover::Quote);
        app.set_mode(Mode::Quote(QuoteLength::Random));
        app.animate_control(ControlHover::Quote, ControlAnimationKind::ActivateIn);
        app.pulse(UiPulseKind::Setting);
        return true;
    }
    if is_safe_typing_screen_binding(app, key, &app.config.keybindings.language) {
        app.control_hover = Some(ControlHover::Language);
        app.language_menu_hover = None;
        app.language_menu_offset = 0;
        open_overlay(app, Overlay::LanguageMenu);
        return true;
    }
    if is_safe_typing_screen_binding(app, key, &app.config.keybindings.amount_left) {
        cycle_main_amount(app, -1);
        return true;
    }
    if is_safe_typing_screen_binding(app, key, &app.config.keybindings.amount_right) {
        cycle_main_amount(app, 1);
        return true;
    }
    if is_safe_typing_screen_binding(app, key, &app.config.keybindings.settings) {
        open_overlay(app, Overlay::Settings);
        return true;
    }
    if is_safe_typing_screen_binding(app, key, &app.config.keybindings.left) {
        move_main_focus(app, -1);
        return true;
    }
    if is_safe_typing_screen_binding(app, key, &app.config.keybindings.right) {
        move_main_focus(app, 1);
        return true;
    }
    if is_safe_typing_screen_binding(app, key, &app.config.keybindings.activate) {
        activate_main_focus(app);
        return true;
    }
    false
}

fn cycle_main_amount(app: &mut App, delta: isize) {
    match &app.session.mode {
        Mode::Time(current) => {
            let choices = app.config.time_mode_choices();
            if let Some(next) = cycle_choice(*current, &choices, delta) {
                let hover = choices
                    .iter()
                    .position(|choice| *choice == next)
                    .map(ControlHover::Amount);
                app.set_mode(Mode::Time(next));
                app.control_hover = hover;
                if let Some(hover) = hover {
                    app.animate_control(hover, ControlAnimationKind::ActivateIn);
                }
                app.pulse(UiPulseKind::Setting);
            }
        }
        Mode::Words(current) => {
            let choices = app.config.word_mode_choices();
            if let Some(next) = cycle_choice(*current, &choices, delta) {
                let hover = choices
                    .iter()
                    .position(|choice| *choice == next)
                    .map(ControlHover::Amount);
                app.set_mode(Mode::Words(next));
                app.control_hover = hover;
                if let Some(hover) = hover {
                    app.animate_control(hover, ControlAnimationKind::ActivateIn);
                }
                app.pulse(UiPulseKind::Setting);
            }
        }
        Mode::Quote(_) => {
            let choices = dictionary_languages();
            if choices.is_empty() {
                return;
            }
            let current = choices
                .iter()
                .position(|choice| choice.eq_ignore_ascii_case(&app.dictionary.language))
                .unwrap_or(0);
            let next = (current as isize + delta).rem_euclid(choices.len() as isize) as usize;
            let language = choices[next].clone();
            app.set_language(&language);
            let hover = ControlHover::Amount(next);
            app.control_hover = Some(hover);
            app.animate_control(hover, ControlAnimationKind::ActivateIn);
            app.pulse(UiPulseKind::Setting);
        }
        _ => {
            app.status = tr(
                app,
                "amount shortcuts work in time, words, and quote modes",
                "быстрый выбор количества работает в режимах времени, слов и цитат",
            )
            .to_string();
        }
    }
}

fn cycle_choice<T: Copy + PartialEq>(current: T, choices: &[T], delta: isize) -> Option<T> {
    if choices.is_empty() {
        return None;
    }
    let current = choices
        .iter()
        .position(|choice| *choice == current)
        .unwrap_or(0);
    Some(choices[(current as isize + delta).rem_euclid(choices.len() as isize) as usize])
}

fn handle_mouse(app: &mut App, mouse: MouseEvent, key_clicks: &mut KeyClickPlayer) {
    key_clicks.set_style(&app.config.key_sound_style);
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) if handle_click(app, mouse.column, mouse.row) => {
            key_clicks.set_style(&app.config.key_sound_style);
            key_clicks.play_mouse();
        }
        MouseEventKind::Down(MouseButton::Left) => {}
        MouseEventKind::Moved if app.overlay == Overlay::LanguageMenu => {
            handle_language_menu_hover(app, mouse.column, mouse.row);
        }
        MouseEventKind::Moved if app.overlay == Overlay::Settings => {
            handle_settings_hover(app, mouse.column, mouse.row);
        }
        MouseEventKind::Moved if app.overlay == Overlay::Results => {
            handle_result_actions_hover(app, mouse.column, mouse.row);
        }
        MouseEventKind::Moved if app.overlay == Overlay::Heatmap => {
            handle_heatmap_hover(app, mouse.column, mouse.row);
        }
        MouseEventKind::Moved if app.overlay == Overlay::History => {
            handle_history_hover(app, mouse.column, mouse.row);
        }
        MouseEventKind::Moved if app.overlay == Overlay::None => {
            handle_control_hover(app, mouse.column, mouse.row);
        }
        MouseEventKind::Moved => {
            app.control_hover = None;
            clear_overlay_hover(app);
        }
        MouseEventKind::ScrollUp if app.overlay == Overlay::LanguageMenu => {
            scroll_language_menu(app, -1);
        }
        MouseEventKind::ScrollDown if app.overlay == Overlay::LanguageMenu => {
            scroll_language_menu(app, 1);
        }
        MouseEventKind::ScrollUp if app.overlay == Overlay::History => {
            scroll_history(app, -1);
        }
        MouseEventKind::ScrollDown if app.overlay == Overlay::History => {
            scroll_history(app, 1);
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            app.status = tr(
                app,
                "text viewport follows the current word",
                "область текста следует за текущим словом",
            )
            .to_string();
        }
        _ => {}
    }
}

fn handle_click(app: &mut App, x: u16, y: u16) -> bool {
    let previous_overlay = app.overlay;
    match app.overlay {
        Overlay::Settings => {
            let clicked = handle_settings_click(app, x, y);
            pulse_after_overlay_click(app, previous_overlay);
            return clicked;
        }
        Overlay::LanguageMenu => {
            let clicked = handle_language_menu_click(app, x, y);
            pulse_after_overlay_click(app, previous_overlay);
            return clicked;
        }
        Overlay::History => {
            let clicked = handle_history_click(app, x, y);
            pulse_after_overlay_click(app, previous_overlay);
            return clicked;
        }
        Overlay::Heatmap => {
            let clicked = handle_heatmap_click(app, x, y);
            pulse_after_overlay_click(app, previous_overlay);
            return clicked;
        }
        Overlay::ConfirmClearHistory => {
            let clicked = handle_clear_history_confirm_click(app, x, y);
            pulse_after_overlay_click(app, previous_overlay);
            return clicked;
        }
        Overlay::Results => {
            if handle_result_action_click(app, x, y) {
                return true;
            }
            return false;
        }
        Overlay::Help | Overlay::Pause => {
            close_overlay(app);
            return true;
        }
        Overlay::None => {}
    }

    let Ok((width, height)) = terminal::size() else {
        return false;
    };
    let area = Rect::new(0, 0, width, height);
    let panels = control_panels(app, area);

    let extra_labels = [
        tr(app, "@ punctuation", "@ пунктуация"),
        tr(app, "# num", "# числа"),
    ];
    if let Some(hit) = label_hit(panels[0], &extra_labels, x, y) {
        match hit {
            0 => {
                let was_active = app.config.punctuation;
                app.control_hover = Some(ControlHover::Punctuation);
                app.toggle_punctuation();
                app.animate_control(
                    ControlHover::Punctuation,
                    if was_active {
                        ControlAnimationKind::ActivateOut
                    } else {
                        ControlAnimationKind::ActivateIn
                    },
                );
            }
            _ => {
                let was_active = app.config.numbers;
                app.control_hover = Some(ControlHover::Numbers);
                app.toggle_numbers();
                app.animate_control(
                    ControlHover::Numbers,
                    if was_active {
                        ControlAnimationKind::ActivateOut
                    } else {
                        ControlAnimationKind::ActivateIn
                    },
                );
            }
        }
        app.pulse(UiPulseKind::Setting);
        return true;
    }

    let mode_labels = [
        tr(app, "time", "время"),
        tr(app, "words", "слова"),
        tr(app, "quote", "цитата"),
    ];
    if let Some(hit) = label_hit(panels[1], &mode_labels, x, y) {
        match hit {
            0 => {
                app.control_hover = Some(ControlHover::Time);
                app.set_mode(Mode::Time(app.config.default_time));
                app.animate_control(ControlHover::Time, ControlAnimationKind::ActivateIn);
            }
            1 => {
                app.control_hover = Some(ControlHover::Words);
                app.set_mode(Mode::Words(app.config.default_words));
                app.animate_control(ControlHover::Words, ControlAnimationKind::ActivateIn);
            }
            _ => {
                app.control_hover = Some(ControlHover::Quote);
                app.set_mode(Mode::Quote(QuoteLength::Random));
                app.animate_control(ControlHover::Quote, ControlAnimationKind::ActivateIn);
            }
        }
        app.pulse(UiPulseKind::Setting);
        return true;
    }

    let amount_labels = amount_labels(app);
    let amount_label_refs: Vec<&str> = amount_labels.iter().map(String::as_str).collect();
    if let Some(hit) = label_hit(panels[2], &amount_label_refs, x, y) {
        app.control_hover = Some(ControlHover::Amount(hit));
        if matches!(&app.session.mode, Mode::Words(_)) {
            if let Some(words) = app.config.word_mode_choices().get(hit).copied() {
                app.set_mode(Mode::Words(words));
            }
        } else if matches!(&app.session.mode, Mode::Quote(_)) {
            if let Some(language) = dictionary_languages().get(hit) {
                app.set_language(language);
            }
        } else if let Some(seconds) = app.config.time_mode_choices().get(hit).copied() {
            app.set_mode(Mode::Time(seconds));
        }
        app.animate_control(ControlHover::Amount(hit), ControlAnimationKind::ActivateIn);
        app.pulse(UiPulseKind::Setting);
        return true;
    }

    let language_label = format!("{} {}", tr(app, "language", "словарь"), app.dictionary.name);
    if label_hit(panels[3], &[&language_label], x, y).is_some() {
        app.control_hover = Some(ControlHover::Language);
        app.language_menu_hover = None;
        app.language_menu_offset = 0;
        open_overlay(app, Overlay::LanguageMenu);
        return true;
    }

    let settings_label = tr(app, "settings", "настройки");
    if label_hit(panels[4], &[settings_label], x, y).is_some() {
        open_overlay(app, Overlay::Settings);
        return true;
    }

    if status_result_button_hit(app, area, x, y) {
        open_overlay(app, Overlay::Results);
        return true;
    }
    false
}

fn handle_language_menu_click(app: &mut App, x: u16, y: u16) -> bool {
    let Ok((width, height)) = terminal::size() else {
        return false;
    };
    let area = Rect::new(0, 0, width, height);
    let names = dictionary_names();
    let rect = language_menu_rect(app, area, names.len());

    if !in_rect(x, y, rect) {
        close_overlay(app);
        app.language_menu_hover = None;
        return true;
    }

    if let Some(idx) = language_menu_index(app, rect, names.len(), y) {
        app.set_dictionary(&names[idx]);
        app.language_menu_hover = None;
        close_overlay(app);
        app.pulse(UiPulseKind::Setting);
        return true;
    }
    false
}

fn handle_language_menu_hover(app: &mut App, x: u16, y: u16) {
    let Ok((width, height)) = terminal::size() else {
        return;
    };
    let names = dictionary_names();
    let rect = language_menu_rect(app, Rect::new(0, 0, width, height), names.len());
    app.language_menu_hover = if in_rect(x, y, rect) {
        language_menu_index(app, rect, names.len(), y)
    } else {
        None
    };
}

fn scroll_language_menu(app: &mut App, delta: isize) {
    let Ok((width, height)) = terminal::size() else {
        return;
    };
    let names = dictionary_names();
    let rect = language_menu_rect(app, Rect::new(0, 0, width, height), names.len());
    let visible = rect.height.saturating_sub(2) as usize;
    let max_offset = names.len().saturating_sub(visible);
    app.language_menu_offset = app
        .language_menu_offset
        .saturating_add_signed(delta)
        .min(max_offset);
}

fn move_language_menu_focus(app: &mut App, delta: isize) {
    let names = dictionary_names();
    if names.is_empty() {
        return;
    }
    let current = app
        .language_menu_hover
        .or_else(|| names.iter().position(|name| *name == app.dictionary.name))
        .unwrap_or(if delta >= 0 { names.len() - 1 } else { 0 });
    let next = (current as isize + delta).rem_euclid(names.len() as isize) as usize;
    app.language_menu_hover = Some(next);

    let Ok((width, height)) = terminal::size() else {
        return;
    };
    let rect = language_menu_rect(app, Rect::new(0, 0, width, height), names.len());
    let visible = rect.height.saturating_sub(2) as usize;
    if next < app.language_menu_offset {
        app.language_menu_offset = next;
    } else if visible > 0 && next >= app.language_menu_offset.saturating_add(visible) {
        app.language_menu_offset = next.saturating_add(1).saturating_sub(visible);
    }
}

fn activate_language_menu_focus(app: &mut App) {
    let names = dictionary_names();
    let Some(idx) = app.language_menu_hover else {
        move_language_menu_focus(app, 1);
        return;
    };
    if let Some(name) = names.get(idx) {
        app.set_dictionary(name);
        app.language_menu_hover = None;
        close_overlay(app);
        app.pulse(UiPulseKind::Setting);
    }
}

fn language_menu_index(app: &App, rect: Rect, total: usize, y: u16) -> Option<usize> {
    let row = y.checked_sub(rect.y + 1)? as usize;
    let idx = app.language_menu_offset.saturating_add(row);
    (row < rect.height.saturating_sub(2) as usize && idx < total).then_some(idx)
}

fn handle_control_hover(app: &mut App, x: u16, y: u16) {
    let Ok((width, height)) = terminal::size() else {
        return;
    };
    let next = control_hover_at(app, Rect::new(0, 0, width, height), x, y);
    set_animated_control_hover(app, next);
}

fn set_animated_control_hover(app: &mut App, next: Option<ControlHover>) {
    if next != app.control_hover {
        let previous = app.control_hover;
        app.control_hover = next;
        if let Some(previous) = previous {
            app.animate_hover_exit(previous);
        }
        if let Some(next) = next {
            app.animate_control(next, ControlAnimationKind::HoverIn);
        }
    }
}

fn handle_result_actions_hover(app: &mut App, x: u16, y: u16) {
    let Ok((width, height)) = terminal::size() else {
        return;
    };
    let next = result_action_hover_at(app, Rect::new(0, 0, width, height), x, y);
    if next != app.control_hover {
        let previous = app.control_hover;
        app.control_hover = next;
        if let Some(previous) = previous
            && matches!(
                previous,
                ControlHover::ResultRestart | ControlHover::ResultRepeat | ControlHover::ResultQuit
            )
        {
            app.animate_hover_exit(previous);
        }
        if let Some(next) = next
            && matches!(
                next,
                ControlHover::ResultRestart | ControlHover::ResultRepeat | ControlHover::ResultQuit
            )
        {
            app.animate_control(next, ControlAnimationKind::HoverIn);
        }
    }
}

fn handle_result_action_click(app: &mut App, x: u16, y: u16) -> bool {
    let Ok((width, height)) = terminal::size() else {
        return false;
    };
    let area = Rect::new(0, 0, width, height);
    let Some(action) = result_action_label_hit(app, result_actions_rect(app, area), x, y) else {
        return false;
    };
    app.control_hover = Some(action);
    app.animate_control(action, ControlAnimationKind::ActivateIn);
    match action {
        ControlHover::ResultRestart => {
            app.restart_new_text();
            app.pulse(UiPulseKind::Restart);
        }
        ControlHover::ResultRepeat => {
            app.retry_same_text();
            app.pulse(UiPulseKind::Restart);
        }
        ControlHover::ResultQuit => {
            close_overlay(app);
            app.pulse(UiPulseKind::Overlay);
        }
        _ => return false,
    }
    true
}

fn control_hover_at(app: &App, area: Rect, x: u16, y: u16) -> Option<ControlHover> {
    let panels = control_panels(app, area);

    let extra_labels = [
        tr(app, "@ punctuation", "@ пунктуация"),
        tr(app, "# num", "# числа"),
    ];
    if let Some(hit) = label_hit(panels[0], &extra_labels, x, y) {
        return Some(match hit {
            0 => ControlHover::Punctuation,
            _ => ControlHover::Numbers,
        });
    }
    let mode_labels = [
        tr(app, "time", "время"),
        tr(app, "words", "слова"),
        tr(app, "quote", "цитата"),
    ];
    if let Some(hit) = label_hit(panels[1], &mode_labels, x, y) {
        return Some(match hit {
            0 => ControlHover::Time,
            1 => ControlHover::Words,
            _ => ControlHover::Quote,
        });
    }
    let amount_labels = amount_labels(app);
    let amount_label_refs: Vec<&str> = amount_labels.iter().map(String::as_str).collect();
    if let Some(hit) = label_hit(panels[2], &amount_label_refs, x, y) {
        return Some(ControlHover::Amount(hit));
    }
    let language_label = format!("{} {}", tr(app, "language", "словарь"), app.dictionary.name);
    if label_hit(panels[3], &[&language_label], x, y).is_some() {
        return Some(ControlHover::Language);
    }
    let settings_label = tr(app, "settings", "настройки");
    if label_hit(panels[4], &[settings_label], x, y).is_some() {
        return Some(ControlHover::Settings);
    }
    if status_result_button_hit(app, area, x, y) {
        return Some(ControlHover::ResultOpen);
    }
    None
}

fn main_keyboard_controls(app: &App) -> Vec<ControlHover> {
    let mut controls = vec![
        ControlHover::Punctuation,
        ControlHover::Numbers,
        ControlHover::Time,
        ControlHover::Words,
        ControlHover::Quote,
    ];
    controls.extend((0..amount_labels(app).len()).map(ControlHover::Amount));
    controls.push(ControlHover::Language);
    controls.push(ControlHover::Settings);
    controls
}

fn move_main_focus(app: &mut App, delta: isize) {
    let controls = main_keyboard_controls(app);
    if controls.is_empty() {
        return;
    }
    let current = app
        .control_hover
        .and_then(|hover| controls.iter().position(|control| *control == hover))
        .unwrap_or(if delta >= 0 { controls.len() - 1 } else { 0 });
    let next = (current as isize + delta).rem_euclid(controls.len() as isize) as usize;
    let previous = app.control_hover;
    app.control_hover = Some(controls[next]);
    if let Some(previous) = previous {
        app.animate_hover_exit(previous);
    }
    app.animate_control(controls[next], ControlAnimationKind::HoverIn);
}

fn activate_main_focus(app: &mut App) {
    let controls = main_keyboard_controls(app);
    let control = app
        .control_hover
        .filter(|hover| controls.contains(hover))
        .unwrap_or(ControlHover::Settings);
    app.control_hover = Some(control);
    match control {
        ControlHover::Punctuation => {
            let was_active = app.config.punctuation;
            app.toggle_punctuation();
            app.animate_control(
                ControlHover::Punctuation,
                if was_active {
                    ControlAnimationKind::ActivateOut
                } else {
                    ControlAnimationKind::ActivateIn
                },
            );
            app.pulse(UiPulseKind::Setting);
        }
        ControlHover::Numbers => {
            let was_active = app.config.numbers;
            app.toggle_numbers();
            app.animate_control(
                ControlHover::Numbers,
                if was_active {
                    ControlAnimationKind::ActivateOut
                } else {
                    ControlAnimationKind::ActivateIn
                },
            );
            app.pulse(UiPulseKind::Setting);
        }
        ControlHover::Time => {
            app.set_mode(Mode::Time(app.config.default_time));
            app.animate_control(ControlHover::Time, ControlAnimationKind::ActivateIn);
            app.pulse(UiPulseKind::Setting);
        }
        ControlHover::Words => {
            app.set_mode(Mode::Words(app.config.default_words));
            app.animate_control(ControlHover::Words, ControlAnimationKind::ActivateIn);
            app.pulse(UiPulseKind::Setting);
        }
        ControlHover::Quote => {
            app.set_mode(Mode::Quote(QuoteLength::Random));
            app.animate_control(ControlHover::Quote, ControlAnimationKind::ActivateIn);
            app.pulse(UiPulseKind::Setting);
        }
        ControlHover::Amount(idx) => {
            if matches!(&app.session.mode, Mode::Words(_)) {
                if let Some(words) = app.config.word_mode_choices().get(idx).copied() {
                    app.set_mode(Mode::Words(words));
                }
            } else if matches!(&app.session.mode, Mode::Quote(_)) {
                if let Some(language) = dictionary_languages().get(idx) {
                    app.set_language(language);
                }
            } else if let Some(seconds) = app.config.time_mode_choices().get(idx).copied() {
                app.set_mode(Mode::Time(seconds));
            }
            app.animate_control(ControlHover::Amount(idx), ControlAnimationKind::ActivateIn);
            app.pulse(UiPulseKind::Setting);
        }
        ControlHover::Language => {
            app.language_menu_hover = None;
            app.language_menu_offset = 0;
            open_overlay(app, Overlay::LanguageMenu);
        }
        ControlHover::Settings => open_overlay(app, Overlay::Settings),
        _ => {}
    }
}

fn result_action_hover_at(app: &App, area: Rect, x: u16, y: u16) -> Option<ControlHover> {
    let actions = result_actions_rect(app, area);
    result_action_label_hit(app, actions, x, y).or_else(|| result_metric_hit(app, area, x, y))
}

fn result_keyboard_actions() -> [ControlHover; 3] {
    [
        ControlHover::ResultRestart,
        ControlHover::ResultRepeat,
        ControlHover::ResultQuit,
    ]
}

fn move_result_focus(app: &mut App, delta: isize) {
    let actions = result_keyboard_actions();
    let current = app
        .control_hover
        .and_then(|hover| actions.iter().position(|action| *action == hover))
        .unwrap_or(if delta >= 0 { actions.len() - 1 } else { 0 });
    let next = (current as isize + delta).rem_euclid(actions.len() as isize) as usize;
    let previous = app.control_hover;
    app.control_hover = Some(actions[next]);
    if let Some(previous) = previous {
        app.animate_hover_exit(previous);
    }
    app.animate_control(actions[next], ControlAnimationKind::HoverIn);
}

fn activate_result_focus(app: &mut App) {
    let actions = result_keyboard_actions();
    let action = app
        .control_hover
        .filter(|hover| actions.contains(hover))
        .unwrap_or(ControlHover::ResultRestart);
    app.control_hover = Some(action);
    app.animate_control(action, ControlAnimationKind::ActivateIn);
    match action {
        ControlHover::ResultRestart => {
            app.restart_new_text();
            app.pulse(UiPulseKind::Restart);
        }
        ControlHover::ResultRepeat => {
            app.retry_same_text();
            app.pulse(UiPulseKind::Restart);
        }
        ControlHover::ResultQuit => {
            close_overlay(app);
            app.pulse(UiPulseKind::Overlay);
        }
        _ => {}
    }
}

fn amount_labels(app: &App) -> Vec<String> {
    match &app.session.mode {
        Mode::Words(_) => app
            .config
            .word_mode_choices()
            .into_iter()
            .map(|words| words.to_string())
            .collect(),
        Mode::Quote(_) => dictionary_languages(),
        _ => app
            .config
            .time_mode_choices()
            .into_iter()
            .map(|seconds| seconds.to_string())
            .collect(),
    }
}

fn control_panels(app: &App, area: Rect) -> [Rect; 5] {
    let scale = app_ui_scale(app);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(3),
        ])
        .split(area);
    let preferred = [
        scaled_dim(25, scale).max(label_group_width(&[
            tr(app, "@ punctuation", "@ пунктуация"),
            tr(app, "# num", "# числа"),
        ])),
        scaled_dim(24, scale).max(label_group_width(&[
            tr(app, "time", "время"),
            tr(app, "words", "слова"),
            tr(app, "quote", "цитата"),
        ])),
        amount_panel_width(app, scale),
        scaled_dim(30, scale),
        scaled_dim(12, scale).max(tr(app, "settings", "настройки").width() as u16),
    ];
    let minimum_width = preferred.iter().copied().sum::<u16>().saturating_add(12);
    let controls_width = scaled_dim(CONTROLS_WIDTH, scale)
        .max(minimum_width)
        .min(chunks[1].width);
    let controls = centered(chunks[1], controls_width, 3);
    proportional_rects(
        controls,
        preferred,
        if controls.width >= minimum_width {
            3
        } else {
            1
        },
    )
}

fn label_group_width(labels: &[&str]) -> u16 {
    labels
        .iter()
        .map(|label| label.width() as u16)
        .sum::<u16>()
        .saturating_add(labels.len().saturating_sub(1) as u16 * 2)
}

fn amount_panel_width(app: &App, scale: u16) -> u16 {
    let labels = amount_labels(app);
    let label_width = labels
        .iter()
        .map(|label| label.width() as u16)
        .sum::<u16>()
        .saturating_add(labels.len().saturating_sub(1) as u16 * 2);
    scaled_dim(22, scale).max(14).max(label_width)
}

fn language_menu_rect(app: &App, area: Rect, item_count: usize) -> Rect {
    let panels = control_panels(app, area);
    let max_visible = area.height.saturating_sub(panels[3].y.saturating_add(5)) as usize;
    let visible = item_count.min(max_visible.max(1)).min(10);
    Rect {
        x: panels[3].x,
        y: panels[3].y.saturating_add(2),
        width: panels[3].width,
        height: visible as u16 + 2,
    }
}

fn handle_settings_click(app: &mut App, x: u16, y: u16) -> bool {
    let Ok((width, height)) = terminal::size() else {
        return false;
    };
    let rect = centered(
        Rect::new(0, 0, width, height),
        scaled_dim(SETTINGS_WIDTH, app_ui_scale(app)),
        scaled_dim(SETTINGS_HEIGHT, app_ui_scale(app)),
    );
    if !in_rect(x, y, rect) {
        close_overlay(app);
        return true;
    }

    if let Some(button) = settings_button_at(rect, x, y) {
        match button {
            SettingsButtonHover::History => open_overlay(app, Overlay::History),
            SettingsButtonHover::Heatmap => open_overlay(app, Overlay::Heatmap),
            SettingsButtonHover::Close => close_overlay(app),
        }
        return true;
    }

    let Some(row) = settings_row_at(rect, y) else {
        return false;
    };
    app.settings_focus = Some(row);

    let changed = adjust_setting_row(app, row, settings_click_delta(app, rect, row, x));
    if !app.should_quit {
        open_overlay(app, Overlay::Settings);
    }
    changed
}

fn adjust_setting_row(app: &mut App, row: usize, delta: isize) -> bool {
    let mut changed = false;
    match row {
        SETTING_THEME => {
            app.cycle_theme_by(delta);
            changed = true;
        }
        SETTING_VISUAL_STYLE => {
            app.cycle_visual_style_by(delta);
            changed = true;
        }
        SETTING_CURSOR_STYLE => {
            app.cycle_cursor_style_by(delta);
            changed = true;
        }
        SETTING_INTERFACE_LANGUAGE => {
            app.cycle_interface_language_by(delta);
            changed = true;
        }
        SETTING_LANGUAGE => {
            app.cycle_language_by(delta);
            changed = true;
        }
        SETTING_PUNCTUATION => {
            app.toggle_punctuation();
            changed = true;
        }
        SETTING_NUMBERS => {
            app.toggle_numbers();
            changed = true;
        }
        SETTING_DIFFICULTY => {
            app.cycle_difficulty_by(delta);
            changed = true;
        }
        SETTING_QUICK_RESTART => {
            app.cycle_quick_restart_by(delta);
            changed = true;
        }
        SETTING_REPEAT_QUOTES => {
            app.cycle_repeat_quotes_by(delta);
            changed = true;
        }
        SETTING_BLIND_MODE => {
            app.toggle_blind_mode();
            changed = true;
        }
        SETTING_WORDS_HISTORY => {
            app.toggle_words_history();
            changed = true;
        }
        SETTING_SPEED_UNIT => {
            app.cycle_speed_unit_by(delta);
            changed = true;
        }
        SETTING_MIN_SPEED => {
            app.cycle_min_speed_by(delta);
            changed = true;
        }
        SETTING_MIN_ACCURACY => {
            app.cycle_min_accuracy_by(delta);
            changed = true;
        }
        SETTING_MIN_WORD_BURST => {
            app.cycle_min_word_burst_by(delta);
            changed = true;
        }
        SETTING_SAVE_RESULTS => {
            app.toggle_save_results();
            changed = true;
        }
        SETTING_KEY_SOUND_STYLE => {
            app.cycle_key_sound_style_by(delta);
            changed = true;
        }
        _ => {}
    }
    if changed {
        let _ = app.config.save();
        app.pulse(UiPulseKind::Setting);
    }
    changed
}

fn settings_click_delta(app: &App, rect: Rect, row: usize, x: u16) -> isize {
    let control = settings_control_rect(app, rect, row);
    if x >= control.x && x < control.x.saturating_add(control.width) {
        let mid = control.x.saturating_add(control.width / 2);
        if x < mid { -1 } else { 1 }
    } else {
        1
    }
}

fn settings_control_rect(app: &App, rect: Rect, row: usize) -> Rect {
    let table = settings_table_rect(app, rect);
    Rect {
        x: table.x.saturating_add(SETTINGS_LABEL_WIDTH as u16),
        y: table.y.saturating_add(row as u16),
        width: SETTINGS_CONTROL_WIDTH as u16,
        height: 1,
    }
}

fn settings_table_rect(app: &App, rect: Rect) -> Rect {
    let inner = Rect {
        x: rect.x.saturating_add(2),
        y: rect.y.saturating_add(1),
        width: rect.width.saturating_sub(4),
        height: rect.height.saturating_sub(2),
    };
    let table_area = Rect {
        x: inner.x,
        y: inner.y.saturating_add(1),
        width: inner.width,
        height: (SETTING_LAST + 1) as u16,
    };
    centered(
        table_area,
        scaled_dim(72, app_ui_scale(app)).min(inner.width),
        table_area.height,
    )
}

fn move_settings_focus(app: &mut App, delta: isize) {
    let current = app
        .settings_focus
        .unwrap_or(if delta >= 0 { SETTING_LAST } else { 0 });
    let next = (current as isize + delta).rem_euclid((SETTING_LAST + 1) as isize) as usize;
    app.settings_focus = Some(next);
}

fn handle_settings_hover(app: &mut App, x: u16, y: u16) {
    let Ok((width, height)) = terminal::size() else {
        return;
    };
    let rect = centered(
        Rect::new(0, 0, width, height),
        scaled_dim(SETTINGS_WIDTH, app_ui_scale(app)),
        scaled_dim(SETTINGS_HEIGHT, app_ui_scale(app)),
    );
    if !in_rect(x, y, rect) {
        app.settings_focus = None;
        app.settings_button_hover = None;
        set_animated_control_hover(app, None);
        return;
    }

    app.settings_button_hover = settings_button_at(rect, x, y);
    app.settings_focus = if app.settings_button_hover.is_none() {
        settings_row_at(rect, y)
    } else {
        None
    };
    set_animated_control_hover(
        app,
        app.settings_button_hover.map(settings_button_control_hover),
    );
}

fn settings_row_at(rect: Rect, y: u16) -> Option<usize> {
    let row = y.checked_sub(rect.y + 2)? as usize;
    (row <= SETTING_LAST).then_some(row)
}

fn setting_description(app: &App, row: Option<usize>) -> &'static str {
    match row {
        Some(SETTING_THEME) => tr(
            app,
            "Changes the full color palette used by the interface and result chart.",
            "Меняет палитру интерфейса и графика результатов.",
        ),
        Some(SETTING_VISUAL_STYLE) => tr(
            app,
            "Changes the decorative terminal art around the trainer.",
            "Меняет декоративный стиль вокруг тренажера.",
        ),
        Some(SETTING_CURSOR_STYLE) => tr(
            app,
            "Changes the cursor shape shown at the active typing position.",
            "Меняет вид курсора в текущей позиции ввода.",
        ),
        Some(SETTING_INTERFACE_LANGUAGE) => tr(
            app,
            "Changes the interface language without changing the typing dictionary.",
            "Меняет язык интерфейса, не меняя словарь для тренировки.",
        ),
        Some(SETTING_LANGUAGE) => tr(
            app,
            "Cycles the active dictionary used to generate words and quotes.",
            "Переключает словарь для слов и цитат.",
        ),
        Some(SETTING_PUNCTUATION) => tr(
            app,
            "Adds punctuation marks to generated word tests.",
            "Добавляет знаки препинания в тесты со словами.",
        ),
        Some(SETTING_NUMBERS) => tr(
            app,
            "Adds numbers to generated word tests.",
            "Добавляет числа в тесты со словами.",
        ),
        Some(SETTING_DIFFICULTY) => tr(
            app,
            "Normal allows corrections, expert fails submitted wrong words, master fails on any wrong key.",
            "Normal позволяет исправления, expert проваливает неверное слово, master - любую ошибку.",
        ),
        Some(SETTING_QUICK_RESTART) => tr(
            app,
            "Chooses which key quickly restarts the current test.",
            "Выбирает клавишу быстрого рестарта текущего теста.",
        ),
        Some(SETTING_REPEAT_QUOTES) => tr(
            app,
            "Controls whether quote restart keeps the current quote while typing.",
            "Управляет повтором текущей цитаты во время набора.",
        ),
        Some(SETTING_BLIND_MODE) => tr(
            app,
            "Hides incorrect input highlighting so you can focus on raw speed.",
            "Скрывает подсветку ошибок, чтобы тренировать чистую скорость.",
        ),
        Some(SETTING_WORDS_HISTORY) => tr(
            app,
            "Automatically opens words history after a completed test.",
            "Автоматически открывает историю слов после завершения теста.",
        ),
        Some(SETTING_SPEED_UNIT) => tr(
            app,
            "Switches speed display and speed thresholds between WPM and CPM.",
            "Переключает скорость и пороги между WPM и CPM.",
        ),
        Some(SETTING_MIN_SPEED) => tr(
            app,
            "Fails a test when current speed drops below this threshold.",
            "Проваливает тест, если скорость падает ниже порога.",
        ),
        Some(SETTING_MIN_ACCURACY) => tr(
            app,
            "Fails a test when accuracy drops below this percentage threshold.",
            "Проваливает тест, если точность падает ниже порога.",
        ),
        Some(SETTING_MIN_WORD_BURST) => tr(
            app,
            "Fails a test when a single word burst is below the selected threshold.",
            "Проваливает тест, если рывок одного слова ниже порога.",
        ),
        Some(SETTING_SAVE_RESULTS) => tr(
            app,
            "Stores finished results in local history.",
            "Сохраняет завершенные результаты в локальную историю.",
        ),
        Some(SETTING_KEY_SOUND_STYLE) => tr(
            app,
            "Chooses one fixed sound profile for typing and mouse controls, or disables it.",
            "Выбирает профиль звука клавиш и мыши или отключает его.",
        ),
        _ => tr(
            app,
            "Hover or click a setting to see what it changes.",
            "Наведите курсор или нажмите на настройку, чтобы увидеть описание.",
        ),
    }
}

fn settings_button_row(rect: Rect) -> u16 {
    rect.y.saturating_add(rect.height).saturating_sub(2)
}

fn settings_button_at(rect: Rect, x: u16, y: u16) -> Option<SettingsButtonHover> {
    if y != settings_button_row(rect) || !in_rect(x, y, rect) {
        return None;
    }

    let third = rect.width / 3;
    if x < rect.x.saturating_add(third) {
        Some(SettingsButtonHover::History)
    } else if x < rect.x.saturating_add(third.saturating_mul(2)) {
        Some(SettingsButtonHover::Heatmap)
    } else {
        Some(SettingsButtonHover::Close)
    }
}

fn settings_button_control_hover(button: SettingsButtonHover) -> ControlHover {
    match button {
        SettingsButtonHover::History => ControlHover::SettingsHistory,
        SettingsButtonHover::Heatmap => ControlHover::SettingsHeatmap,
        SettingsButtonHover::Close => ControlHover::SettingsClose,
    }
}

fn handle_heatmap_click(app: &mut App, x: u16, y: u16) -> bool {
    let Ok((width, height)) = terminal::size() else {
        return false;
    };
    let rect = heatmap_rect(app, Rect::new(0, 0, width, height));
    if !in_rect(x, y, rect) {
        open_overlay(app, Overlay::Settings);
        return true;
    }

    let [language_area, _, _, hint_area] = heatmap_sections(rect);

    if let Some(language) = heatmap_language_at(app, language_area, x, y) {
        app.set_heatmap_language(&language);
        app.pulse(UiPulseKind::Setting);
        return true;
    }

    if in_rect(x, y, hint_area) {
        open_overlay(app, Overlay::Settings);
        return true;
    }
    false
}

fn handle_heatmap_hover(app: &mut App, x: u16, y: u16) {
    let Ok((width, height)) = terminal::size() else {
        return;
    };
    let rect = heatmap_rect(app, Rect::new(0, 0, width, height));
    let [language_area, _, keyboard_area, _] = heatmap_sections(rect);
    app.heatmap_hover_key = heatmap_key_at(&app.heatmap_language, keyboard_area, x, y);
    app.heatmap_hover_language = heatmap_language_at(app, language_area, x, y);
}

fn handle_history_click(app: &mut App, x: u16, y: u16) -> bool {
    let Ok((width, height)) = terminal::size() else {
        return false;
    };
    let rect = history_rect(app, Rect::new(0, 0, width, height));
    if !in_rect(x, y, rect) {
        open_overlay(app, Overlay::Settings);
        return true;
    }

    let [filters, list, _, _] = history_sections(rect);
    let table = history_table_rect(list);
    if let Some(next) = history_filter_at(app, filters, x, y) {
        let hovered_filter = history_filter_index_at(app, filters, x, y);
        app.history_language_filter = next;
        app.history_offset = 0;
        app.history_selected = 0;
        app.history_hover = None;
        app.history_filter_hover = hovered_filter;
        app.status = history_filter_status(app);
        return true;
    }

    if let Some(row) = history_row_at(app, table, x, y) {
        app.history_selected = row;
        ensure_history_selection_visible(app);
        app.status = tr(app, "selected saved test", "выбран сохраненный тест").to_string();
        return true;
    }

    if let Some(action) = history_action_at(rect, x, y) {
        match action {
            HistoryActionHover::Clear => open_overlay(app, Overlay::ConfirmClearHistory),
            HistoryActionHover::Back => open_overlay(app, Overlay::Settings),
        }
        return true;
    }
    false
}

fn handle_history_hover(app: &mut App, x: u16, y: u16) {
    let Ok((width, height)) = terminal::size() else {
        return;
    };
    let rect = history_rect(app, Rect::new(0, 0, width, height));
    let [filters, list, _, _] = history_sections(rect);
    app.history_hover = history_row_at(app, history_table_rect(list), x, y);
    app.history_filter_hover = history_filter_index_at(app, filters, x, y);
    app.history_action_hover = history_action_at(rect, x, y);
    let next = app
        .history_action_hover
        .map(history_action_control_hover)
        .or_else(|| app.history_filter_hover.map(ControlHover::HistoryFilter));
    set_animated_control_hover(app, next);
}

fn history_filter_at(app: &App, area: Rect, x: u16, y: u16) -> Option<Option<String>> {
    let filters = history_language_filters(app);
    history_filter_index_at(app, area, x, y).and_then(|idx| filters.get(idx).cloned())
}

fn history_filter_index_at(app: &App, area: Rect, x: u16, y: u16) -> Option<usize> {
    if y != area.y {
        return None;
    }
    let filters = history_language_filters(app);
    let labels = filters
        .iter()
        .map(|filter| {
            if app.history_language_filter == *filter {
                format!("[{}]", history_language_label(app, filter.as_deref()))
            } else {
                format!(" {} ", history_language_label(app, filter.as_deref()))
            }
        })
        .collect::<Vec<_>>();
    let total_width = labels.iter().map(|label| label.width()).sum::<usize>()
        + labels.len().saturating_sub(1) * 2;
    let mut cursor = area
        .x
        .saturating_add(area.width.saturating_sub(total_width as u16) / 2);
    for (idx, label) in labels.into_iter().enumerate() {
        let end = cursor.saturating_add(label.width() as u16);
        if x >= cursor && x < end {
            return Some(idx);
        }
        cursor = end.saturating_add(2);
    }
    None
}

fn history_action_at(rect: Rect, x: u16, y: u16) -> Option<HistoryActionHover> {
    let actions = history_actions_rect(rect);
    if !in_rect(x, y, actions) {
        return None;
    }

    let [clear_button, _, _, back_button] = Layout::horizontal([
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
    ])
    .areas(actions);
    if in_rect(x, y, clear_button) {
        Some(HistoryActionHover::Clear)
    } else if in_rect(x, y, back_button) {
        Some(HistoryActionHover::Back)
    } else {
        None
    }
}

fn history_action_control_hover(action: HistoryActionHover) -> ControlHover {
    match action {
        HistoryActionHover::Clear => ControlHover::HistoryClear,
        HistoryActionHover::Back => ControlHover::HistoryBack,
    }
}

fn history_row_at(app: &App, area: Rect, x: u16, y: u16) -> Option<usize> {
    if !in_rect(x, y, area) || y <= area.y {
        return None;
    }
    let row = y.saturating_sub(area.y).saturating_sub(1) as usize;
    if row >= history_visible_rows_for(area) {
        return None;
    }
    let idx = app.history_offset.saturating_add(row);
    let count = app
        .storage
        .result_count(app.history_language_filter.as_deref(), None)
        .ok()?;
    (idx < count).then_some(idx)
}

fn history_visible_rows(app: &App) -> usize {
    let Ok((width, height)) = terminal::size() else {
        return 1;
    };
    let rect = history_rect(app, Rect::new(0, 0, width, height));
    history_visible_rows_for(history_table_rect(history_sections(rect)[1]))
}

fn move_history_selection(app: &mut App, delta: isize) {
    let Ok(total) = app
        .storage
        .result_count(app.history_language_filter.as_deref(), None)
    else {
        return;
    };
    if total == 0 {
        app.history_offset = 0;
        app.history_selected = 0;
        return;
    }
    let next =
        (app.history_selected as isize + delta).clamp(0, total.saturating_sub(1) as isize) as usize;
    app.history_selected = next;
    ensure_history_selection_visible(app);
}

fn scroll_history(app: &mut App, delta: isize) {
    let Ok(total) = app
        .storage
        .result_count(app.history_language_filter.as_deref(), None)
    else {
        return;
    };
    let visible = history_visible_rows(app);
    let max_offset = total.saturating_sub(visible);
    app.history_offset =
        (app.history_offset as isize + delta).clamp(0, max_offset as isize) as usize;
}

fn ensure_history_selection_visible(app: &mut App) {
    let Ok(total) = app
        .storage
        .result_count(app.history_language_filter.as_deref(), None)
    else {
        return;
    };
    if total == 0 {
        app.history_offset = 0;
        app.history_selected = 0;
        return;
    }
    app.history_selected = app.history_selected.min(total.saturating_sub(1));
    let visible = history_visible_rows(app);
    let max_offset = total.saturating_sub(visible);
    if app.history_selected < app.history_offset {
        app.history_offset = app.history_selected;
    } else if app.history_selected >= app.history_offset.saturating_add(visible) {
        app.history_offset = app
            .history_selected
            .saturating_add(1)
            .saturating_sub(visible);
    }
    app.history_offset = app.history_offset.min(max_offset);
}

fn cycle_history_language(app: &mut App, delta: isize) {
    let filters = history_language_filters(app);
    if filters.is_empty() {
        return;
    }
    let current = filters
        .iter()
        .position(|filter| *filter == app.history_language_filter)
        .unwrap_or(0);
    let next = (current as isize + delta).rem_euclid(filters.len() as isize) as usize;
    app.history_language_filter = filters[next].clone();
    app.history_offset = 0;
    app.history_selected = 0;
    app.history_hover = None;
    app.history_filter_hover = None;
    app.history_action_hover = None;
    app.status = history_filter_status(app);
}

fn handle_clear_history_confirm_click(app: &mut App, x: u16, y: u16) -> bool {
    let Ok((width, height)) = terminal::size() else {
        return false;
    };
    let rect = confirm_clear_history_rect(app, Rect::new(0, 0, width, height));
    if !in_rect(x, y, rect) {
        open_overlay(app, Overlay::History);
        return true;
    }

    let actions = confirm_clear_history_actions_rect(rect);
    let [delete_button, cancel_button] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(actions);
    if in_rect(x, y, delete_button) {
        clear_history(app);
        return true;
    } else if in_rect(x, y, cancel_button) {
        open_overlay(app, Overlay::History);
        return true;
    }
    false
}

fn clear_history(app: &mut App) {
    match app.storage.clear_results() {
        Ok(deleted) => {
            app.history_offset = 0;
            app.history_selected = 0;
            app.history_hover = None;
            app.history_filter_hover = None;
            app.history_action_hover = None;
            app.status = format!(
                "{} {deleted} {}",
                tr(app, "deleted", "удалено"),
                tr(app, "saved tests", "сохраненных тестов")
            );
            open_overlay(app, Overlay::History);
        }
        Err(err) => {
            app.status = format!(
                "{}: {err}",
                tr(app, "history delete failed", "не удалось удалить историю")
            );
            open_overlay(app, Overlay::History);
        }
    }
}

fn handle_overlay_key(app: &mut App, key: KeyEvent) {
    match app.overlay {
        Overlay::Pause => {
            if is_quick_restart_key(app, key) {
                app.restart_requested();
            } else if key_binding_matches(key, &app.config.keybindings.activate)
                || key_binding_matches(key, &app.config.keybindings.close)
                || key_binding_matches(key, &app.config.keybindings.pause)
            {
                app.session.resume();
                close_overlay(app);
            }
        }
        Overlay::Results => {
            if key_binding_matches(key, &app.config.keybindings.mode_time) {
                app.control_hover = Some(ControlHover::Time);
                app.set_mode(Mode::Time(app.config.default_time));
                app.animate_control(ControlHover::Time, ControlAnimationKind::ActivateIn);
                app.pulse(UiPulseKind::Setting);
            } else if key_binding_matches(key, &app.config.keybindings.mode_words) {
                app.control_hover = Some(ControlHover::Words);
                app.set_mode(Mode::Words(app.config.default_words));
                app.animate_control(ControlHover::Words, ControlAnimationKind::ActivateIn);
                app.pulse(UiPulseKind::Setting);
            } else if key_binding_matches(key, &app.config.keybindings.mode_quote) {
                app.control_hover = Some(ControlHover::Quote);
                app.set_mode(Mode::Quote(QuoteLength::Random));
                app.animate_control(ControlHover::Quote, ControlAnimationKind::ActivateIn);
                app.pulse(UiPulseKind::Setting);
            } else if key_binding_matches(key, &app.config.keybindings.retry_text) {
                app.retry_same_text();
                app.pulse(UiPulseKind::Restart);
            } else if is_quick_restart_key(app, key) {
                app.restart_requested();
                app.pulse(UiPulseKind::Restart);
            } else if key_binding_matches(key, &app.config.keybindings.close) {
                close_overlay(app);
            } else if key_binding_matches(key, &app.config.keybindings.left) {
                move_result_focus(app, -1);
            } else if key_binding_matches(key, &app.config.keybindings.right) {
                move_result_focus(app, 1);
            } else if key_binding_matches(key, &app.config.keybindings.activate) {
                activate_result_focus(app);
            }
        }
        Overlay::History => {
            if key_binding_matches(key, &app.config.keybindings.close) {
                open_overlay(app, Overlay::Settings);
            } else if key_binding_matches(key, &app.config.keybindings.delete_history) {
                open_overlay(app, Overlay::ConfirmClearHistory);
            } else if key_binding_matches(key, &app.config.keybindings.down) {
                move_history_selection(app, 1);
            } else if key_binding_matches(key, &app.config.keybindings.up) {
                move_history_selection(app, -1);
            } else if key_binding_matches(key, &app.config.keybindings.right) {
                cycle_history_language(app, 1);
            } else if key_binding_matches(key, &app.config.keybindings.left) {
                cycle_history_language(app, -1);
            } else if key_binding_matches(key, &app.config.keybindings.activate) {
                ensure_history_selection_visible(app);
                app.status = tr(app, "selected saved test", "выбран сохраненный тест").to_string();
            }
        }
        Overlay::Heatmap => {
            if key_binding_matches(key, &app.config.keybindings.close) {
                open_overlay(app, Overlay::Settings);
            } else if key_binding_matches(key, &app.config.keybindings.right) {
                app.cycle_heatmap_language(1);
            } else if key_binding_matches(key, &app.config.keybindings.left) {
                app.cycle_heatmap_language(-1);
            }
        }
        Overlay::ConfirmClearHistory => {
            if key_binding_matches(key, &app.config.keybindings.confirm) {
                clear_history(app);
            } else if key_binding_matches(key, &app.config.keybindings.cancel)
                || key_binding_matches(key, &app.config.keybindings.close)
            {
                open_overlay(app, Overlay::History)
            }
        }
        Overlay::Help => {
            if key_binding_matches(key, &app.config.keybindings.close)
                || key_binding_matches(key, &app.config.keybindings.help)
            {
                close_overlay(app);
            }
        }
        Overlay::Settings => {
            if key_binding_matches(key, &app.config.keybindings.settings)
                || key_binding_matches(key, &app.config.keybindings.close)
            {
                close_overlay(app);
            } else if key_binding_matches(key, &app.config.keybindings.down) {
                move_settings_focus(app, 1);
            } else if key_binding_matches(key, &app.config.keybindings.up) {
                move_settings_focus(app, -1);
            } else if key_binding_matches(key, &app.config.keybindings.left) {
                let row = app.settings_focus.unwrap_or(0);
                app.settings_focus = Some(row);
                if adjust_setting_row(app, row, -1) && !app.should_quit {
                    open_overlay(app, Overlay::Settings);
                }
            } else if key_binding_matches(key, &app.config.keybindings.activate)
                || key_binding_matches(key, &app.config.keybindings.right)
            {
                let row = app.settings_focus.unwrap_or(0);
                app.settings_focus = Some(row);
                if adjust_setting_row(app, row, 1) && !app.should_quit {
                    open_overlay(app, Overlay::Settings);
                }
            } else if key_binding_matches(key, &app.config.keybindings.history) {
                open_overlay(app, Overlay::History);
            } else if key_binding_matches(key, &app.config.keybindings.heatmap) {
                open_overlay(app, Overlay::Heatmap);
            }
        }
        Overlay::LanguageMenu => {
            if key_binding_matches(key, &app.config.keybindings.close) {
                close_overlay(app);
            } else if key_binding_matches(key, &app.config.keybindings.down) {
                move_language_menu_focus(app, 1);
            } else if key_binding_matches(key, &app.config.keybindings.up) {
                move_language_menu_focus(app, -1);
            } else if key_binding_matches(key, &app.config.keybindings.activate) {
                activate_language_menu_focus(app);
            }
        }
        Overlay::None => {}
    }
}

fn pulse_after_interaction(app: &mut App, previous_overlay: Overlay, previous_input_len: usize) {
    if app
        .ui_pulse
        .is_some_and(|pulse| pulse.started_at.elapsed() < Duration::from_millis(25))
    {
        return;
    }
    if app.overlay != previous_overlay {
        app.pulse(UiPulseKind::Overlay);
    } else if app.session.input.len() != previous_input_len {
        app.pulse(UiPulseKind::Input);
    }
}

fn pulse_after_overlay_click(app: &mut App, previous_overlay: Overlay) {
    if app.overlay != previous_overlay {
        app.pulse(UiPulseKind::Overlay);
    } else if app
        .ui_pulse
        .is_none_or(|pulse| pulse.started_at.elapsed() >= Duration::from_millis(25))
    {
        app.pulse(UiPulseKind::Click);
    }
}

fn open_overlay(app: &mut App, overlay: Overlay) {
    clear_overlay_hover(app);
    app.overlay = overlay;
    app.pulse(UiPulseKind::Overlay);
}

fn close_overlay(app: &mut App) {
    if app.overlay != Overlay::None {
        clear_overlay_hover(app);
        app.overlay = Overlay::None;
    }
}

fn clear_overlay_hover(app: &mut App) {
    app.settings_button_hover = None;
    app.control_hover = None;
    app.history_hover = None;
    app.history_filter_hover = None;
    app.history_action_hover = None;
    app.heatmap_hover_key = None;
    app.heatmap_hover_language = None;
}

fn is_quick_restart_key(app: &App, key: KeyEvent) -> bool {
    key_binding_matches(key, &app.config.keybindings.restart)
}

fn is_safe_typing_screen_binding(app: &App, key: KeyEvent, binding: &str) -> bool {
    key_binding_matches(key, binding) && !key_can_edit_text(app, key)
}

fn key_can_edit_text(app: &App, key: KeyEvent) -> bool {
    if app.overlay != Overlay::None
        || matches!(app.session.state, TestState::Finished | TestState::Failed)
    {
        return false;
    }
    let text_modifiers = key.modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT);
    if !text_modifiers.is_empty() {
        return false;
    }
    matches!(
        key.code,
        KeyCode::Char(_) | KeyCode::Enter | KeyCode::Backspace
    )
}

fn key_binding_matches(key: KeyEvent, binding: &str) -> bool {
    binding
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .any(|part| key_spec_matches(key, part))
}

fn key_spec_matches(key: KeyEvent, spec: &str) -> bool {
    let mut modifiers = KeyModifiers::NONE;
    let mut key_part = None;
    for part in spec
        .split('+')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
    {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
            "alt" => modifiers |= KeyModifiers::ALT,
            "shift" => modifiers |= KeyModifiers::SHIFT,
            raw => key_part = Some(raw.to_string()),
        }
    }

    let Some(key_part) = key_part else {
        return false;
    };
    let Some(code) = parse_key_code(&key_part) else {
        return false;
    };
    key_code_matches(key.code, code, modifiers.contains(KeyModifiers::SHIFT))
        && modifier_matches(key, modifiers)
}

fn parse_key_code(value: &str) -> Option<KeyCode> {
    Some(match value {
        "esc" | "escape" => KeyCode::Esc,
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "space" => KeyCode::Char(' '),
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        raw if raw.len() > 1 && raw.starts_with('f') => {
            let number = raw[1..].parse::<u8>().ok()?;
            if !(1..=24).contains(&number) {
                return None;
            }
            KeyCode::F(number)
        }
        raw if raw.chars().count() == 1 => KeyCode::Char(raw.chars().next()?),
        _ => return None,
    })
}

fn key_code_matches(actual: KeyCode, expected: KeyCode, shifted: bool) -> bool {
    match (actual, expected) {
        (KeyCode::Char(actual), KeyCode::Char(expected)) if shifted => {
            actual.eq_ignore_ascii_case(&expected)
        }
        (KeyCode::Char(actual), KeyCode::Char(expected)) => actual == expected,
        _ => actual == expected,
    }
}

fn modifier_matches(key: KeyEvent, expected: KeyModifiers) -> bool {
    let relevant = KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT;
    let mut actual = key.modifiers & relevant;
    if matches!(key.code, KeyCode::Char(_)) && !expected.contains(KeyModifiers::SHIFT) {
        actual.remove(KeyModifiers::SHIFT);
    }
    actual == expected
}

fn render(frame: &mut Frame, app: &App) {
    let theme = &app.theme;
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        area,
    );
    render_visual_art(frame, app, area);

    if app.overlay == Overlay::Results {
        render_results(frame, app, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(3),
        ])
        .split(area);

    render_header(frame, app, chunks[0]);
    render_controls(frame, app, area);
    render_typing_area(frame, app, chunks[2]);
    render_status(frame, app, chunks[3]);

    if app.overlay != Overlay::None {
        render_overlay(frame, app, area, app.overlay);
    }
}

fn render_overlay(frame: &mut Frame, app: &App, area: Rect, overlay: Overlay) {
    match overlay {
        Overlay::Pause => render_pause(frame, app, area),
        Overlay::Help => render_help(frame, app, area),
        Overlay::Settings => render_settings(frame, app, area),
        Overlay::LanguageMenu => render_language_menu(frame, app, area),
        Overlay::History => render_history(frame, app, area),
        Overlay::Heatmap => render_heatmap(frame, app, area),
        Overlay::ConfirmClearHistory => render_clear_history_confirm(frame, app, area),
        Overlay::Results | Overlay::None => {}
    }
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let logo = Line::from(vec![
        Span::styled(
            "[tt] ",
            Style::default().fg(theme.main).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "term",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled("typist", Style::default().fg(theme.text)),
    ]);
    frame.render_widget(
        Paragraph::new(logo).alignment(Alignment::Left),
        inset(area, 2, 1),
    );
}

fn render_visual_art(frame: &mut Frame, app: &App, area: Rect) {
    if area.width < 48 || area.height < 12 {
        return;
    }

    match app.config.visual_style.as_str() {
        "space" => render_space_art(frame, app, area),
        "stardust" | "neon" => render_stardust_art(frame, app, area),
        "fireflies" | "matrix" => render_fireflies_art(frame, app, area),
        "snowfall" | "paper" => render_snowfall_art(frame, app, area),
        "embers" => render_embers_art(frame, app, area),
        _ => {}
    }
}

fn render_space_art(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    render_static_field(
        frame,
        area,
        theme,
        ParticleSpec {
            count: 22,
            x_mul: 17,
            y_mul: 7,
            x_offset: 5,
            y_offset: 3,
            glyph: ".",
            accent: "*",
            accent_every: 6,
            region: ParticleRegion::Full,
        },
    );
}

fn render_stardust_art(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    render_static_field(
        frame,
        area,
        theme,
        ParticleSpec {
            count: 34,
            x_mul: 9,
            y_mul: 4,
            x_offset: 2,
            y_offset: 1,
            glyph: ".",
            accent: ":",
            accent_every: 4,
            region: ParticleRegion::Upper,
        },
    );
}

fn render_fireflies_art(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    render_static_field(
        frame,
        area,
        theme,
        ParticleSpec {
            count: 18,
            x_mul: 23,
            y_mul: 5,
            x_offset: 7,
            y_offset: 6,
            glyph: ".",
            accent: "*",
            accent_every: 3,
            region: ParticleRegion::Sides,
        },
    );
}

fn render_snowfall_art(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    render_static_field(
        frame,
        area,
        theme,
        ParticleSpec {
            count: 30,
            x_mul: 7,
            y_mul: 6,
            x_offset: 4,
            y_offset: 1,
            glyph: ".",
            accent: "+",
            accent_every: 8,
            region: ParticleRegion::Full,
        },
    );
}

fn render_embers_art(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    render_static_field(
        frame,
        area,
        theme,
        ParticleSpec {
            count: 24,
            x_mul: 19,
            y_mul: 5,
            x_offset: 3,
            y_offset: 2,
            glyph: ".",
            accent: "*",
            accent_every: 4,
            region: ParticleRegion::Lower,
        },
    );
}

#[derive(Clone, Copy)]
struct ParticleSpec {
    count: usize,
    x_mul: usize,
    y_mul: usize,
    x_offset: usize,
    y_offset: usize,
    glyph: &'static str,
    accent: &'static str,
    accent_every: usize,
    region: ParticleRegion,
}

#[derive(Clone, Copy)]
enum ParticleRegion {
    Full,
    Upper,
    Lower,
    Sides,
}

fn render_static_field(
    frame: &mut Frame,
    area: Rect,
    theme: &crate::themes::ResolvedTheme,
    spec: ParticleSpec,
) {
    for idx in 0..spec.count {
        let (x, y) = particle_position(area, idx, spec);
        let accented = idx % spec.accent_every == 0;
        let color = if accented { theme.main } else { theme.muted };
        render_art_text(
            frame,
            x,
            y,
            if accented { spec.accent } else { spec.glyph },
            color,
        );
    }
}

fn particle_position(area: Rect, idx: usize, spec: ParticleSpec) -> (u16, u16) {
    let width = area.width.max(1) as usize;
    let height = area.height.max(1) as usize;
    let base_x = (idx * spec.x_mul + spec.x_offset) % width;
    let base_y = (idx * spec.y_mul + spec.y_offset) % height;
    let (x, y) = match spec.region {
        ParticleRegion::Full => (base_x, base_y),
        ParticleRegion::Upper => (base_x, base_y % (height / 2).max(1)),
        ParticleRegion::Lower => (base_x, height / 2 + base_y % (height / 2).max(1)),
        ParticleRegion::Sides => {
            let side_width = (width / 5).max(2);
            let x = if idx.is_multiple_of(2) {
                base_x % side_width
            } else {
                width.saturating_sub(1 + base_x % side_width)
            };
            (x, base_y)
        }
    };
    (
        area.x.saturating_add(x as u16),
        area.y.saturating_add(y as u16),
    )
}

fn animation_level(app: &App) -> usize {
    let _ = app;
    1
}

fn pulse_border_style(app: &App, fallback: Color) -> Style {
    let _ = app;
    Style::default().fg(fallback)
}

fn transition_rect(app: &App, area: Rect, width: u16, height: u16) -> Rect {
    let _ = app;
    centered(area, width, height)
}

fn render_border_runner(frame: &mut Frame, app: &App, rect: Rect, title: Option<&str>) {
    if rect.width < 3 || rect.height < 3 || animation_level(app) == 0 {
        return;
    }
    let Some(pulse) = app.ui_pulse else {
        return;
    };
    if !matches!(
        pulse.kind,
        UiPulseKind::Click | UiPulseKind::Overlay | UiPulseKind::Setting | UiPulseKind::Restart
    ) {
        return;
    }

    let duration = BORDER_ANIMATION_MS;
    let elapsed = pulse.started_at.elapsed().as_millis() as f64;
    if elapsed >= duration {
        return;
    }

    let perimeter = rect
        .width
        .saturating_mul(2)
        .saturating_add(rect.height.saturating_sub(2).saturating_mul(2));
    if perimeter == 0 {
        return;
    }

    let progress = elapsed / duration;
    let filled = ((progress * perimeter as f64).round() as u16).clamp(1, perimeter);
    for idx in 0..perimeter {
        if idx > filled {
            continue;
        }
        let (x, y, glyph) = border_point(rect, idx);
        if title.is_some_and(|title| in_title_span(rect, x, y, title)) {
            continue;
        }
        let head_distance = filled.saturating_sub(idx);
        let color = if head_distance < 4 {
            app.theme.text
        } else {
            app.theme.main
        };
        render_art_styled(
            frame,
            x,
            y,
            glyph,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        );
    }
}

fn in_title_span(rect: Rect, x: u16, y: u16, title: &str) -> bool {
    if y != rect.y {
        return false;
    }
    let start = rect.x.saturating_add(1);
    let end = start.saturating_add(title.len() as u16).saturating_add(1);
    x >= start && x <= end
}

fn border_point(rect: Rect, idx: u16) -> (u16, u16, &'static str) {
    let top = rect.width;
    let right = rect.height.saturating_sub(2);
    let bottom = rect.width;

    if idx < top {
        let x = rect.x.saturating_add(idx);
        let glyph = if idx == 0 {
            "┌"
        } else if idx == top.saturating_sub(1) {
            "┐"
        } else {
            "─"
        };
        return (x, rect.y, glyph);
    }

    let idx = idx.saturating_sub(top);
    if idx < right {
        return (
            rect.x.saturating_add(rect.width.saturating_sub(1)),
            rect.y.saturating_add(1 + idx),
            "│",
        );
    }

    let idx = idx.saturating_sub(right);
    if idx < bottom {
        let x = rect
            .x
            .saturating_add(rect.width.saturating_sub(1).saturating_sub(idx));
        let glyph = if idx == 0 {
            "┘"
        } else if idx == bottom.saturating_sub(1) {
            "└"
        } else {
            "─"
        };
        return (
            x,
            rect.y.saturating_add(rect.height.saturating_sub(1)),
            glyph,
        );
    }

    let idx = idx.saturating_sub(bottom);
    (
        rect.x,
        rect.y
            .saturating_add(rect.height.saturating_sub(2).saturating_sub(idx)),
        "│",
    )
}

fn render_art_text(frame: &mut Frame, x: u16, y: u16, text: &str, color: Color) {
    render_art_styled(frame, x, y, text, Style::default().fg(color));
}

fn render_art_styled(frame: &mut Frame, x: u16, y: u16, text: &str, style: Style) {
    let bounds = frame.area();
    if y < bounds.y
        || y >= bounds.y.saturating_add(bounds.height)
        || x < bounds.x
        || x >= bounds.x.saturating_add(bounds.width)
    {
        return;
    }
    let width = (text.len() as u16).min(bounds.x.saturating_add(bounds.width).saturating_sub(x));
    if width == 0 {
        return;
    }
    let line = Line::from(Span::styled(text.to_string(), style));
    frame.render_widget(
        Paragraph::new(line),
        Rect {
            x,
            y,
            width,
            height: 1,
        },
    );
}

fn push_chip(
    spans: &mut Vec<Span<'static>>,
    app: &App,
    control: ControlHover,
    label: &str,
    active: bool,
    hovered: bool,
    colors: (Color, Color),
) {
    spans.extend(chip(
        app, control, label, active, hovered, colors.0, colors.1,
    ));
}

fn render_controls(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let chunks = control_panels(app, area);

    let mut extras = Vec::new();
    push_chip(
        &mut extras,
        app,
        ControlHover::Punctuation,
        tr(app, "@ punctuation", "@ пунктуация"),
        app.config.punctuation,
        app.control_hover == Some(ControlHover::Punctuation),
        (theme.main, theme.muted),
    );
    extras.push(Span::raw("  "));
    push_chip(
        &mut extras,
        app,
        ControlHover::Numbers,
        tr(app, "# num", "# числа"),
        app.config.numbers,
        app.control_hover == Some(ControlHover::Numbers),
        (theme.main, theme.muted),
    );
    let extras = Line::from(extras);
    frame.render_widget(panel(extras, theme), chunks[0]);

    let mut mode = Vec::new();
    push_chip(
        &mut mode,
        app,
        ControlHover::Time,
        tr(app, "time", "время"),
        matches!(&app.session.mode, Mode::Time(_)),
        app.control_hover == Some(ControlHover::Time),
        (theme.main, theme.muted),
    );
    mode.push(Span::raw("  "));
    push_chip(
        &mut mode,
        app,
        ControlHover::Words,
        tr(app, "words", "слова"),
        matches!(&app.session.mode, Mode::Words(_)),
        app.control_hover == Some(ControlHover::Words),
        (theme.main, theme.muted),
    );
    mode.push(Span::raw("  "));
    push_chip(
        &mut mode,
        app,
        ControlHover::Quote,
        tr(app, "quote", "цитата"),
        matches!(&app.session.mode, Mode::Quote(_)),
        app.control_hover == Some(ControlHover::Quote),
        (theme.main, theme.muted),
    );
    let mode = Line::from(mode);
    frame.render_widget(panel(mode, theme), chunks[1]);

    let amount_specs = match &app.session.mode {
        Mode::Words(current) => Some(
            app.config
                .word_mode_choices()
                .into_iter()
                .map(|words| (words.to_string(), *current == words))
                .collect::<Vec<_>>(),
        ),
        Mode::Quote(_) => Some(
            dictionary_languages()
                .into_iter()
                .map(|language| {
                    let active = language.eq_ignore_ascii_case(&app.dictionary.language);
                    (language, active)
                })
                .collect::<Vec<_>>(),
        ),
        Mode::Time(current) => Some(
            app.config
                .time_mode_choices()
                .into_iter()
                .map(|seconds| (seconds.to_string(), *current == seconds))
                .collect::<Vec<_>>(),
        ),
        Mode::Custom(_) | Mode::LastConfig => Some(
            app.config
                .time_mode_choices()
                .into_iter()
                .map(|seconds| (seconds.to_string(), false))
                .collect::<Vec<_>>(),
        ),
    };
    let mut amounts = Vec::new();
    if let Some(amount_specs) = amount_specs {
        for (idx, (label, active)) in amount_specs.iter().enumerate() {
            if idx > 0 {
                amounts.push(Span::raw("  "));
            }
            push_chip(
                &mut amounts,
                app,
                ControlHover::Amount(idx),
                label,
                *active,
                app.control_hover == Some(ControlHover::Amount(idx)),
                (theme.main, theme.muted),
            );
        }
    }
    let amounts = Line::from(amounts);
    frame.render_widget(panel(amounts, theme), chunks[2]);

    let mut language = chip(
        app,
        ControlHover::Language,
        tr(app, "language ", "словарь "),
        false,
        app.control_hover == Some(ControlHover::Language),
        theme.main,
        theme.muted,
    );
    language.push(Span::styled(
        app.dictionary.name.clone(),
        hover_style(
            app.control_hover == Some(ControlHover::Language),
            false,
            theme.text,
            theme.muted,
        ),
    ));
    let language = Line::from(language);
    frame.render_widget(panel(language, theme), chunks[3]);

    let settings = Line::from(chip(
        app,
        ControlHover::Settings,
        tr(app, "settings", "настройки"),
        false,
        app.control_hover == Some(ControlHover::Settings),
        theme.main,
        theme.muted,
    ));
    frame.render_widget(panel(settings, theme), chunks[4]);
}

fn render_typing_area(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let inner = typing_area_rect(app, area);
    let metric = app.session.metrics();
    let mut lines = Vec::new();

    let language = format!(
        "{}   {}   {:.0} {}   {:.1}% acc",
        app.dictionary.name,
        mode_label_ui(app, &app.session.mode),
        speed_value(&app.config, &metric),
        speed_unit(&app.config),
        metric.accuracy
    );
    lines.push(Line::from(Span::styled(language, Style::default().fg(theme.muted))).centered());
    lines.push(Line::raw(""));

    lines.extend(with_line_spacing(wrap_styled_words(
        app,
        inner.width.saturating_sub(2),
        3,
    )));

    let paragraph = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(theme.background));
    frame.render_widget(paragraph, inner);
}

fn typing_area_rect(app: &App, area: Rect) -> Rect {
    centered(
        area,
        scaled_dim(TEXT_WIDTH, app_ui_scale(app)),
        scaled_dim(15, app_ui_scale(app)),
    )
}

fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let progress = app.session.progress();
    let [bar, help] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(2)]).areas(inset(area, 2, 0));
    let progress_width = 72;
    let filled = (progress * progress_width as f64).round() as usize;
    let empty = progress_width as usize - filled.min(progress_width as usize);
    let progress_line = Line::from(vec![
        Span::styled("─".repeat(filled), Style::default().fg(theme.main)),
        Span::styled("─".repeat(empty), Style::default().fg(theme.muted)),
    ]);
    frame.render_widget(
        Paragraph::new(progress_line)
            .alignment(Alignment::Center)
            .style(Style::default().bg(theme.background)),
        centered(bar, progress_width, 1),
    );

    frame.render_widget(
        Paragraph::new(Line::from(status_spans(app)))
            .style(Style::default().fg(theme.muted))
            .alignment(Alignment::Center),
        help,
    );
}

fn status_spans(app: &App) -> Vec<Span<'static>> {
    let theme = &app.theme;
    let mut spans = vec![Span::styled(
        status_prefix(app),
        Style::default().fg(theme.muted),
    )];
    if can_open_result_from_status(app) {
        spans.push(Span::raw("    "));
        push_chip(
            &mut spans,
            app,
            ControlHover::ResultOpen,
            tr(app, "results", "результаты"),
            false,
            app.control_hover == Some(ControlHover::ResultOpen),
            (theme.main, theme.muted),
        );
    }
    spans
}

fn status_prefix(app: &App) -> String {
    let metric = app.session.metrics();
    let label = match app.session.remaining() {
        Some(remaining) => format!("{}{}", remaining.as_secs(), tr(app, "s left", "с осталось")),
        None => format!("{:.0}s", app.session.elapsed().as_secs_f64()),
    };
    format!(
        "{}    {} {}    {} {}    {}",
        label,
        app.config.keybindings.help,
        tr(app, "help", "помощь"),
        metric.errors,
        tr(app, "errors", "ошибок"),
        app.status
    )
}

fn can_open_result_from_status(app: &App) -> bool {
    matches!(app.session.state, TestState::Finished | TestState::Failed)
        && app.overlay == Overlay::None
}

fn status_result_button_hit(app: &App, area: Rect, x: u16, y: u16) -> bool {
    if !can_open_result_from_status(app) {
        return false;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(3),
        ])
        .split(area);
    let [_, help] = Layout::vertical([Constraint::Length(1), Constraint::Length(2)])
        .areas(inset(chunks[3], 2, 0));
    if y != help.y {
        return false;
    }
    let prefix = status_prefix(app);
    let results_label = tr(app, "results", "результаты");
    let total_width = prefix.width() as u16 + 4 + results_label.width() as u16;
    let start = help
        .x
        .saturating_add(help.width.saturating_sub(total_width) / 2);
    let button_start = start
        .saturating_add(prefix.width() as u16)
        .saturating_add(4);
    x >= button_start && x < button_start.saturating_add(results_label.width() as u16)
}

fn render_pause(frame: &mut Frame, app: &App, area: Rect) {
    let restart_key = app.config.keybindings.restart.clone();
    modal(
        frame,
        app,
        area,
        tr(app, "paused", "пауза"),
        vec![
            Line::from(tr(app, "enter / esc  resume", "enter / esc  продолжить")),
            Line::from(format!(
                "{restart_key:<12} {}",
                tr(app, "restart", "рестарт")
            )),
            Line::from(format!("ctrl+c       {}", tr(app, "quit", "выход"))),
        ],
        40,
        7,
    );
}

fn render_help(frame: &mut Frame, app: &App, area: Rect) {
    let keys = &app.config.keybindings;
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                tr(app, "action                 ", "действие               "),
                Style::default()
                    .fg(app.theme.muted)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                tr(
                    app,
                    "keys                         ",
                    "клавиши                      ",
                ),
                Style::default()
                    .fg(app.theme.muted)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                tr(app, "scope", "где"),
                Style::default()
                    .fg(app.theme.muted)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        help_rule(app),
        help_row(
            app,
            tr(app, "restart", "рестарт"),
            &keys.restart,
            tr(app, "typing, results, pause", "набор, результаты, пауза"),
        ),
        help_row(
            app,
            tr(app, "retry same text", "повтор текста"),
            &keys.retry_text,
            tr(app, "typing, results", "набор, результаты"),
        ),
        help_row(
            app,
            tr(app, "pause", "пауза"),
            &keys.pause,
            tr(app, "typing", "набор"),
        ),
        help_row(
            app,
            tr(app, "settings", "настройки"),
            &keys.settings,
            tr(app, "before test", "до теста"),
        ),
        help_row(
            app,
            tr(app, "help", "помощь"),
            &keys.help,
            tr(app, "global", "везде"),
        ),
        help_row(
            app,
            tr(app, "quit", "выход"),
            &keys.quit,
            tr(app, "global", "везде"),
        ),
        help_row(
            app,
            tr(app, "close / back", "закрыть / назад"),
            &keys.close,
            tr(app, "overlays", "окна"),
        ),
        help_rule(app),
        help_row(
            app,
            tr(app, "navigate / adjust", "навигация / смена"),
            &navigation_keys(keys),
            tr(app, "focus, rows, values", "фокус, строки, значения"),
        ),
        help_row(
            app,
            tr(app, "activate", "активировать"),
            &keys.activate,
            tr(app, "focused control", "выбранный элемент"),
        ),
        help_rule(app),
        help_row(
            app,
            tr(app, "punctuation", "пунктуация"),
            &keys.toggle_punctuation,
            tr(app, "before test", "до теста"),
        ),
        help_row(
            app,
            tr(app, "numbers", "числа"),
            &keys.toggle_numbers,
            tr(app, "before test", "до теста"),
        ),
        help_row(
            app,
            tr(app, "time mode", "режим времени"),
            &keys.mode_time,
            tr(app, "before/after test", "до/после теста"),
        ),
        help_row(
            app,
            tr(app, "words mode", "режим слов"),
            &keys.mode_words,
            tr(app, "before/after test", "до/после теста"),
        ),
        help_row(
            app,
            tr(app, "quote mode", "режим цитат"),
            &keys.mode_quote,
            tr(app, "before/after test", "до/после теста"),
        ),
        help_row(
            app,
            tr(app, "language", "словарь"),
            &keys.language,
            tr(app, "before test", "до теста"),
        ),
        help_row(
            app,
            tr(app, "amount", "количество"),
            &format!("{} / {}", keys.amount_left, keys.amount_right),
            tr(app, "time, words, quote", "время, слова, цитата"),
        ),
    ];
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        tr(
            app,
            "typing screen: printable keybindings are typed as text. Edit [keybindings] in config.toml.",
            "на экране набора печатные клавиши вводятся как текст. Меняйте [keybindings] в config.toml.",
        ),
        Style::default().fg(app.theme.muted),
    )));
    modal(frame, app, area, tr(app, "help", "помощь"), lines, 96, 32);
}

fn navigation_keys(keys: &crate::config::KeyBindings) -> String {
    format!(
        "{} {} {} {}",
        first_binding(&keys.left),
        first_binding(&keys.down),
        first_binding(&keys.up),
        first_binding(&keys.right)
    )
}

fn first_binding(binding: &str) -> String {
    binding
        .split(',')
        .map(str::trim)
        .find(|part| !part.is_empty())
        .unwrap_or(binding)
        .to_string()
}

fn help_row(app: &App, action: &str, keys: &str, scope: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{action:<23}"), Style::default().fg(app.theme.text)),
        Span::styled(
            format!("{keys:<29}"),
            Style::default()
                .fg(app.theme.main)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(scope.to_string(), Style::default().fg(app.theme.muted)),
    ])
}

fn help_rule(app: &App) -> Line<'static> {
    Line::from(Span::styled(
        "─".repeat(84),
        Style::default().fg(app.theme.muted),
    ))
}

fn render_settings(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let active = |row| app.settings_focus == Some(row);
    let lines = vec![
        setting_value_row(
            tr(app, "theme", "тема"),
            &app.theme.name,
            tr(app, "click to cycle", "нажмите для смены"),
            theme,
            active(SETTING_THEME),
        ),
        setting_value_row(
            tr(app, "art style", "стиль"),
            &app.config.visual_style,
            tr(app, "space / stardust / etc", "space / stardust / ..."),
            theme,
            active(SETTING_VISUAL_STYLE),
        ),
        setting_value_row(
            tr(app, "cursor", "курсор"),
            &app.config.cursor_style,
            tr(
                app,
                "block / underline / color",
                "block / underline / color",
            ),
            theme,
            active(SETTING_CURSOR_STYLE),
        ),
        setting_value_row(
            tr(app, "interface", "интерфейс"),
            &setting_value_ui(app, interface_language_label_for_config(&app.config)),
            tr(app, "english / russian", "english / русский"),
            theme,
            active(SETTING_INTERFACE_LANGUAGE),
        ),
        setting_value_row(
            tr(app, "language", "словарь"),
            &app.dictionary.name,
            tr(app, "click to cycle", "нажмите для смены"),
            theme,
            active(SETTING_LANGUAGE),
        ),
        setting_toggle_row(
            tr(app, "punctuation", "пунктуация"),
            app.config.punctuation,
            tr(app, "adds punctuation", "добавляет знаки"),
            theme,
            active(SETTING_PUNCTUATION),
            &app.config,
        ),
        setting_toggle_row(
            tr(app, "numbers", "числа"),
            app.config.numbers,
            tr(app, "adds numbers", "добавляет числа"),
            theme,
            active(SETTING_NUMBERS),
            &app.config,
        ),
        setting_value_row(
            tr(app, "difficulty", "сложность"),
            &app.config.difficulty,
            "normal / expert / master",
            theme,
            active(SETTING_DIFFICULTY),
        ),
        setting_value_row(
            tr(app, "quick restart", "быстрый рестарт"),
            &app.config.keybindings.restart,
            "tab / esc / enter",
            theme,
            active(SETTING_QUICK_RESTART),
        ),
        setting_value_row(
            tr(app, "repeat quotes", "повтор цитат"),
            &setting_value_ui(app, &app.config.repeat_quotes),
            tr(app, "off / typing", "выкл / набор"),
            theme,
            active(SETTING_REPEAT_QUOTES),
        ),
        setting_toggle_row(
            tr(app, "blind mode", "слепой режим"),
            app.config.blind_mode,
            tr(app, "hide incorrect input", "скрыть ошибки"),
            theme,
            active(SETTING_BLIND_MODE),
            &app.config,
        ),
        setting_toggle_row(
            tr(app, "words history", "история слов"),
            app.config.always_show_words_history,
            tr(app, "show after test", "после теста"),
            theme,
            active(SETTING_WORDS_HISTORY),
            &app.config,
        ),
        setting_value_row(
            tr(app, "speed unit", "скорость"),
            speed_unit(&app.config),
            "wpm / cpm",
            theme,
            active(SETTING_SPEED_UNIT),
        ),
        setting_value_row(
            tr(app, "min speed", "мин. скорость"),
            &setting_value_ui(app, &numeric_setting(app.config.min_speed)),
            speed_threshold_hint(&app.config),
            theme,
            active(SETTING_MIN_SPEED),
        ),
        setting_value_row(
            tr(app, "min accuracy", "мин. точность"),
            &setting_value_ui(app, &numeric_setting(app.config.min_accuracy)),
            tr(app, "accuracy threshold", "порог точности"),
            theme,
            active(SETTING_MIN_ACCURACY),
        ),
        setting_value_row(
            tr(app, "min word burst", "мин. рывок"),
            &setting_value_ui(app, &app.config.min_word_burst),
            tr(app, "off / flex / value", "выкл / гибко / число"),
            theme,
            active(SETTING_MIN_WORD_BURST),
        ),
        setting_toggle_row(
            tr(app, "save results", "сохр. результаты"),
            app.config.save_results,
            tr(app, "store local history", "локальная история"),
            theme,
            active(SETTING_SAVE_RESULTS),
            &app.config,
        ),
        setting_value_row(
            tr(app, "key sound", "звук клавиш"),
            &setting_value_ui(app, &app.config.key_sound_style),
            tr(app, "off / mechanical / etc", "off / mechanical / ..."),
            theme,
            active(SETTING_KEY_SOUND_STYLE),
        ),
    ];
    settings_modal(
        frame,
        app,
        area,
        lines,
        scaled_dim(SETTINGS_WIDTH, app_ui_scale(app)),
        scaled_dim(SETTINGS_HEIGHT, app_ui_scale(app)),
        setting_description(app, app.settings_focus),
    );
}

fn render_language_menu(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let names = dictionary_names();
    let rect = language_menu_rect(app, area, names.len());
    frame.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(pulse_border_style(app, theme.main))
        .style(Style::default().fg(theme.text).bg(theme.background));
    frame.render_widget(block, rect);
    render_border_runner(frame, app, rect, None);

    let visible = rect.height.saturating_sub(2) as usize;
    let lines = names
        .iter()
        .enumerate()
        .skip(app.language_menu_offset)
        .take(visible)
        .map(|(idx, name)| {
            let active = *name == app.dictionary.name;
            let hovered = app.language_menu_hover == Some(idx);
            let style = hover_style(hovered, active, theme.text, theme.muted);
            Line::from(Span::styled(name.clone(), style)).centered()
        })
        .collect::<Vec<_>>();

    let inner = Rect {
        x: rect.x.saturating_add(1),
        y: rect.y.saturating_add(1),
        width: rect.width.saturating_sub(2),
        height: rect.height.saturating_sub(2),
    };
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.text).bg(theme.background)),
        inner,
    );
}

fn render_history(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let rect = history_rect(app, area);
    frame.render_widget(Clear, rect);

    let block = Block::default()
        .title(tr(app, "test history", "история тестов"))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.main))
        .style(Style::default().fg(theme.text).bg(theme.background));
    frame.render_widget(block, rect);
    render_border_runner(
        frame,
        app,
        rect,
        Some(tr(app, "test history", "история тестов")),
    );

    let [filters, list, chart, actions] = history_sections(rect);
    let table = history_table_rect(list);
    frame.render_widget(
        Paragraph::new(Line::from(history_filter_spans(app)))
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.muted).bg(theme.background)),
        filters,
    );

    let visible = history_visible_rows_for(table);
    let language = app.history_language_filter.as_deref();
    let total = app.storage.result_count(language, None).unwrap_or(0);
    let max_offset = total.saturating_sub(visible);
    let offset = app.history_offset.min(max_offset);
    let selected = app.history_selected.min(total.saturating_sub(1));

    let mut lines = vec![Line::from(vec![Span::styled(
        format!(
            "id   date             mode       lang       {} raw acc err",
            speed_unit(&app.config)
        ),
        Style::default().fg(theme.muted),
    )])];

    match app
        .storage
        .recent_results_page(visible.max(1), offset, language, None)
    {
        Ok(rows) if rows.is_empty() => {
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                tr(
                    app,
                    "no saved tests for this category",
                    "нет сохраненных тестов для этой категории",
                ),
                Style::default().fg(theme.muted),
            )));
        }
        Ok(rows) => {
            for (row_idx, row) in rows.iter().enumerate() {
                let global_idx = offset.saturating_add(row_idx);
                lines.push(history_row_line(
                    app,
                    row,
                    global_idx,
                    global_idx == selected,
                ));
            }
            if total > 0 {
                let from = offset.saturating_add(1).min(total);
                let to = offset.saturating_add(rows.len()).min(total);
                lines.push(Line::from(Span::styled(
                    format!(
                        "{} {from}-{to} {} {total}",
                        tr(app, "showing", "показано"),
                        tr(app, "of", "из")
                    ),
                    Style::default().fg(theme.muted),
                )));
            }
        }
        Err(_) => {
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                tr(
                    app,
                    "failed to read local history",
                    "не удалось прочитать локальную историю",
                ),
                Style::default().fg(theme.error),
            )));
        }
    }

    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.text).bg(theme.background)),
        table,
    );

    render_history_chart(frame, app, chart, selected, total);

    render_history_actions(frame, app, actions);
}

fn history_rect(app: &App, area: Rect) -> Rect {
    centered(
        area,
        scaled_dim(128, app_ui_scale(app)),
        scaled_dim(36, app_ui_scale(app)),
    )
}

fn history_sections(rect: Rect) -> [Rect; 4] {
    let content = Rect {
        x: rect.x.saturating_add(2),
        y: rect.y.saturating_add(1),
        width: rect.width.saturating_sub(4),
        height: rect.height.saturating_sub(2),
    };
    let [filters, body, actions] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(2),
    ])
    .areas(content);

    let [list, chart] = if body.width >= 112 && body.height >= 16 {
        Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)])
            .spacing(2)
            .areas(body)
    } else {
        Layout::vertical([Constraint::Length(body.height.min(12)), Constraint::Fill(1)])
            .spacing(1)
            .areas(body)
    };

    [filters, list, chart, actions]
}

fn history_visible_rows_for(list: Rect) -> usize {
    list.height.saturating_sub(2).max(1) as usize
}

fn history_table_rect(list: Rect) -> Rect {
    centered(list, HISTORY_TABLE_WIDTH.min(list.width), list.height)
}

fn history_filter_spans(app: &App) -> Vec<Span<'static>> {
    let theme = &app.theme;
    let mut spans = Vec::new();
    for (idx, filter) in history_language_filters(app).into_iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw("  "));
        }
        let active = app.history_language_filter == filter;
        let hovered = app.history_filter_hover == Some(idx);
        let label = history_language_label(app, filter.as_deref());
        let label = if active {
            format!("[{label}]")
        } else {
            format!(" {label} ")
        };
        spans.extend(chip(
            app,
            ControlHover::HistoryFilter(idx),
            &label,
            active,
            hovered,
            theme.main,
            theme.muted,
        ));
    }
    spans
}

fn history_language_filters(app: &App) -> Vec<Option<String>> {
    let mut filters = vec![None];
    if let Ok(languages) = app.storage.result_languages() {
        for language in languages {
            if !filters
                .iter()
                .any(|filter| filter.as_deref() == Some(language.as_str()))
            {
                filters.push(Some(language));
            }
        }
    }
    if let Some(active) = &app.history_language_filter
        && !filters
            .iter()
            .any(|filter| filter.as_deref() == Some(active.as_str()))
    {
        filters.push(Some(active.clone()));
    }
    filters
}

fn history_language_label(app: &App, language: Option<&str>) -> String {
    match language {
        None => tr(app, "all", "все").to_string(),
        Some("en") => heatmap_language_label_for_config(&app.config, "en").to_string(),
        Some("ru") => heatmap_language_label_for_config(&app.config, "ru").to_string(),
        Some(value) => value.to_string(),
    }
}

fn history_filter_status(app: &App) -> String {
    format!(
        "{}: {}",
        tr(app, "history", "история"),
        history_language_label(app, app.history_language_filter.as_deref())
    )
}

fn history_row_line(
    app: &App,
    row: &ResultRow,
    global_idx: usize,
    selected: bool,
) -> Line<'static> {
    let theme = &app.theme;
    let speed = row_speed_value(&app.config, row.wpm);
    let raw_speed = row_speed_value(&app.config, row.raw_wpm);
    let hovered = app.history_hover == Some(global_idx);
    let style = if selected {
        Style::default()
            .fg(theme.background)
            .bg(theme.main)
            .add_modifier(Modifier::BOLD)
    } else if hovered {
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text)
    };
    Line::from(Span::styled(
        format!(
            "{:<4} {:<16} {:<10} {:<10} {:>3.0} {:>3.0} {:>5.1}% {:>3}",
            row.id,
            compact_date(&row.created_at),
            row.mode,
            row.language,
            speed,
            raw_speed,
            row.accuracy,
            row.errors
        ),
        style,
    ))
}

fn render_history_chart(frame: &mut Frame, app: &App, area: Rect, selected: usize, total: usize) {
    let theme = &app.theme;
    if total == 0 {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    tr(app, "select a saved test", "выберите сохраненный тест"),
                    Style::default().fg(theme.muted),
                ))
                .centered(),
                Line::from(Span::styled(
                    tr(
                        app,
                        "the result graph will appear here",
                        "здесь появится график результата",
                    ),
                    Style::default().fg(theme.muted),
                ))
                .centered(),
            ])
            .style(Style::default().bg(theme.background)),
            centered(area, area.width, 4),
        );
        return;
    }

    let language = app.history_language_filter.as_deref();
    match app
        .storage
        .recent_results_page(1, selected.min(total.saturating_sub(1)), language, None)
    {
        Ok(rows) => {
            let Some(row) = rows.first() else {
                return;
            };
            let samples = app.storage.result_samples(row.id).unwrap_or_default();
            let samples = if samples.is_empty() {
                fallback_history_samples(row)
            } else {
                samples
            };
            let title = Line::from(vec![
                Span::styled(
                    format!("#{} ", row.id),
                    Style::default().fg(theme.main).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        "{}  {}  {:.1}%  {:.0}s",
                        row.mode, row.language, row.accuracy, row.duration_sec
                    ),
                    Style::default().fg(theme.muted),
                ),
            ]);
            let [title_area, chart_area] =
                Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
            frame.render_widget(
                Paragraph::new(title)
                    .alignment(Alignment::Center)
                    .style(Style::default().bg(theme.background)),
                title_area,
            );
            render_result_chart_from_samples(
                frame,
                app,
                chart_area,
                &samples,
                row_speed_value(&app.config, row.wpm),
            );
        }
        Err(_) => {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    tr(
                        app,
                        "failed to read selected test",
                        "не удалось прочитать выбранный тест",
                    ),
                    Style::default().fg(theme.error),
                )))
                .alignment(Alignment::Center)
                .style(Style::default().bg(theme.background)),
                area,
            );
        }
    }
}

fn fallback_history_samples(row: &ResultRow) -> Vec<SpeedSample> {
    vec![SpeedSample {
        second: row.duration_sec.max(1.0),
        wpm: row.wpm,
        raw_wpm: row.raw_wpm,
        errors: row.errors as f64,
    }]
}

fn history_actions_rect(rect: Rect) -> Rect {
    history_sections(rect)[3]
}

fn render_history_actions(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let clear_hovered = app.history_action_hover == Some(HistoryActionHover::Clear);
    let back_hovered = app.history_action_hover == Some(HistoryActionHover::Back);
    let [clear_button, select_hint, language_hint, back_button] = Layout::horizontal([
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(chip(
                app,
                ControlHover::HistoryClear,
                &app.config.keybindings.delete_history,
                false,
                clear_hovered,
                theme.error,
                theme.muted,
            )),
            Line::from(chip(
                app,
                ControlHover::HistoryClear,
                tr(app, "clear history", "очистить историю"),
                false,
                clear_hovered,
                theme.error,
                theme.muted,
            )),
        ])
        .alignment(Alignment::Center)
        .style(Style::default().bg(theme.background)),
        clear_button,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!(
                    "{}/{}",
                    app.config.keybindings.up, app.config.keybindings.down
                ),
                Style::default().fg(theme.main),
            )),
            Line::from(Span::styled(
                tr(app, "select test", "выбрать тест"),
                Style::default().fg(theme.muted),
            )),
        ])
        .alignment(Alignment::Center)
        .style(Style::default().bg(theme.background)),
        select_hint,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!(
                    "{}/{}",
                    app.config.keybindings.left, app.config.keybindings.right
                ),
                Style::default().fg(theme.main),
            )),
            Line::from(Span::styled(
                tr(app, "language", "язык"),
                Style::default().fg(theme.muted),
            )),
        ])
        .alignment(Alignment::Center)
        .style(Style::default().bg(theme.background)),
        language_hint,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(chip(
                app,
                ControlHover::HistoryBack,
                &app.config.keybindings.close,
                false,
                back_hovered,
                theme.main,
                theme.muted,
            )),
            Line::from(chip(
                app,
                ControlHover::HistoryBack,
                tr(app, "back to settings", "назад к настройкам"),
                false,
                back_hovered,
                theme.main,
                theme.muted,
            )),
        ])
        .alignment(Alignment::Center)
        .style(Style::default().bg(theme.background)),
        back_button,
    );
}

fn render_clear_history_confirm(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let rect = confirm_clear_history_rect(app, area);
    frame.render_widget(Clear, rect);

    let block = Block::default()
        .title(tr(app, "delete history", "удалить историю"))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.error))
        .style(Style::default().fg(theme.text).bg(theme.background));
    frame.render_widget(block, rect);
    render_border_runner(
        frame,
        app,
        rect,
        Some(tr(app, "delete history", "удалить историю")),
    );

    let [message, actions] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)])
        .areas(confirm_clear_history_inner_rect(rect));
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                tr(
                    app,
                    "Delete all saved test history?",
                    "Удалить всю сохраненную историю тестов?",
                ),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ))
            .centered(),
            Line::raw(""),
            Line::from(Span::styled(
                tr(
                    app,
                    "This also clears the global keyboard heatmap data.",
                    "Это также очистит общую тепловую карту клавиатуры.",
                ),
                Style::default().fg(theme.muted),
            ))
            .centered(),
        ])
        .alignment(Alignment::Center)
        .style(Style::default().bg(theme.background)),
        centered(message, message.width, 5),
    );

    let [delete_button, cancel_button] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(actions);
    frame.render_widget(
        Paragraph::new(format!(
            "{}  {}",
            app.config.keybindings.confirm,
            tr(app, "delete", "удалить")
        ))
        .alignment(Alignment::Center)
        .style(Style::default().fg(theme.error).bg(theme.background)),
        delete_button,
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{}  {}",
            app.config.keybindings.cancel,
            tr(app, "cancel", "отмена")
        ))
        .alignment(Alignment::Center)
        .style(Style::default().fg(theme.muted).bg(theme.background)),
        cancel_button,
    );
}

fn confirm_clear_history_rect(app: &App, area: Rect) -> Rect {
    centered(
        area,
        scaled_dim(58, app_ui_scale(app)),
        scaled_dim(10, app_ui_scale(app)),
    )
}

fn confirm_clear_history_inner_rect(rect: Rect) -> Rect {
    Rect {
        x: rect.x.saturating_add(2),
        y: rect.y.saturating_add(1),
        width: rect.width.saturating_sub(4),
        height: rect.height.saturating_sub(2),
    }
}

fn confirm_clear_history_actions_rect(rect: Rect) -> Rect {
    Layout::vertical([Constraint::Fill(1), Constraint::Length(1)])
        .split(confirm_clear_history_inner_rect(rect))[1]
}

fn render_heatmap(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let rect = heatmap_rect(app, area);
    frame.render_widget(Clear, rect);

    let block = Block::default()
        .title(tr(app, "keyboard heatmap", "тепловая карта клавиатуры"))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.main))
        .style(Style::default().fg(theme.text).bg(theme.background));
    frame.render_widget(block, rect);
    render_border_runner(
        frame,
        app,
        rect,
        Some(tr(app, "keyboard heatmap", "тепловая карта клавиатуры")),
    );

    let [language_area, summary_area, keyboard_area, hint_area] = heatmap_sections(rect);

    frame.render_widget(
        Paragraph::new(Line::from(heatmap_language_spans(app)))
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.muted).bg(theme.background)),
        language_area,
    );

    match app.storage.keyboard_heatmap(&app.heatmap_language) {
        Ok(heatmap) if heatmap.tests == 0 => {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        tr(
                            app,
                            "no saved tests for this language",
                            "нет сохраненных тестов для этого языка",
                        ),
                        Style::default().fg(theme.muted),
                    ))
                    .centered(),
                    Line::from(Span::styled(
                        tr(
                            app,
                            "finish a test with save results enabled",
                            "завершите тест с включенным сохранением результатов",
                        ),
                        Style::default().fg(theme.muted),
                    ))
                    .centered(),
                ])
                .style(Style::default().bg(theme.background)),
                centered(keyboard_area, keyboard_area.width, 4),
            );
        }
        Ok(heatmap) => {
            let max_errors = heatmap.keys.iter().map(|key| key.errors).max().unwrap_or(0);
            let counts = heatmap
                .keys
                .iter()
                .map(|key| (key.key.as_str(), key.errors))
                .collect::<std::collections::HashMap<_, _>>();
            let summary = heatmap_summary(app, &counts, heatmap.tests, heatmap.total_errors);
            frame.render_widget(
                Paragraph::new(summary)
                    .alignment(Alignment::Center)
                    .style(Style::default().bg(theme.background)),
                summary_area,
            );

            render_heatmap_keyboard(
                frame,
                app,
                keyboard_area,
                &counts,
                max_errors,
                heatmap.total_errors,
            );
        }
        Err(_) => {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    tr(
                        app,
                        "failed to read local history",
                        "не удалось прочитать локальную историю",
                    ),
                    Style::default().fg(theme.error),
                )))
                .alignment(Alignment::Center)
                .style(Style::default().bg(theme.background)),
                keyboard_area,
            );
        }
    }

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(
                    "{}/{}",
                    app.config.keybindings.left, app.config.keybindings.right
                ),
                Style::default().fg(theme.main),
            ),
            Span::styled(
                format!(" {}    ", tr(app, "switch language", "сменить язык")),
                Style::default().fg(theme.muted),
            ),
            Span::styled(
                app.config.keybindings.close.clone(),
                Style::default().fg(theme.main),
            ),
            Span::styled(
                format!(" {}", tr(app, "back to settings", "назад к настройкам")),
                Style::default().fg(theme.muted),
            ),
        ]))
        .alignment(Alignment::Center)
        .style(Style::default().bg(theme.background)),
        hint_area,
    );
}

fn heatmap_rect(app: &App, area: Rect) -> Rect {
    centered(
        area,
        scaled_dim(106, app_ui_scale(app)),
        scaled_dim(34, app_ui_scale(app)),
    )
}

fn heatmap_sections(rect: Rect) -> [Rect; 4] {
    let inner = Rect {
        x: rect.x.saturating_add(2),
        y: rect.y.saturating_add(1),
        width: rect.width.saturating_sub(4),
        height: rect.height.saturating_sub(2),
    };
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner)
}

fn heatmap_language_spans(app: &App) -> Vec<Span<'static>> {
    let theme = &app.theme;
    let mut spans = Vec::new();
    for (idx, code) in ["en", "ru"].into_iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw("   "));
        }
        let active = app.heatmap_language == code;
        let hovered = app.heatmap_hover_language.as_deref() == Some(code);
        spans.push(Span::styled(
            heatmap_language_label_for_config(&app.config, code).to_string(),
            hover_style(hovered, active, theme.main, theme.muted),
        ));
    }
    spans
}

fn heatmap_language_at(app: &App, area: Rect, x: u16, y: u16) -> Option<String> {
    if y != area.y || x < area.x || x >= area.x.saturating_add(area.width) {
        return None;
    }

    let labels = [
        ("en", heatmap_language_label_for_config(&app.config, "en")),
        ("ru", heatmap_language_label_for_config(&app.config, "ru")),
    ];
    let total_width = labels
        .iter()
        .map(|(_, label)| label.width() as u16)
        .sum::<u16>()
        .saturating_add(3);
    let mut cursor = area
        .x
        .saturating_add(area.width.saturating_sub(total_width) / 2);

    for (idx, (code, label)) in labels.iter().enumerate() {
        let width = label.width() as u16;
        let start = cursor.saturating_sub(1);
        let end = cursor.saturating_add(width).saturating_add(1);
        if x >= start && x < end {
            return Some((*code).to_string());
        }
        cursor = cursor
            .saturating_add(width)
            .saturating_add(if idx == 0 { 3 } else { 0 });
    }
    None
}

#[derive(Clone, Copy)]
struct HeatmapKeySpec {
    key: &'static str,
    label: &'static str,
    width: u16,
}

#[derive(Clone)]
struct HeatmapRowSpec {
    offset: u16,
    keys: Vec<HeatmapKeySpec>,
}

fn heatmap_summary(
    app: &App,
    counts: &std::collections::HashMap<&str, usize>,
    tests: usize,
    total_errors: usize,
) -> Vec<Line<'static>> {
    let theme = &app.theme;
    let base = Line::from(Span::styled(
        format!(
            "{tests} {}   {total_errors} {}",
            tr(app, "tests", "тестов"),
            tr(app, "key errors", "ошибок клавиш")
        ),
        Style::default().fg(theme.muted),
    ));
    let detail = if let Some(key) = app.heatmap_hover_key.as_deref() {
        let errors = counts.get(key).copied().unwrap_or(0);
        let share = if total_errors == 0 {
            0.0
        } else {
            errors as f64 / total_errors as f64 * 100.0
        };
        Line::from(vec![
            Span::styled(
                format!("{} ", tr(app, "key", "клавиша")),
                Style::default().fg(theme.muted),
            ),
            Span::styled(
                heatmap_display_key(key),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "   {errors} {}   {share:.1}% {}",
                    tr(app, "errors", "ошибок"),
                    tr(app, "of heatmap", "карты")
                ),
                Style::default().fg(theme.muted),
            ),
        ])
    } else {
        Line::from(Span::styled(
            tr(
                app,
                "hover a key to inspect its errors",
                "наведите курсор на клавишу, чтобы увидеть ошибки",
            ),
            Style::default().fg(theme.muted),
        ))
    };
    vec![base, detail]
}

fn render_heatmap_keyboard(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    counts: &std::collections::HashMap<&str, usize>,
    max_errors: usize,
    total_errors: usize,
) {
    let theme = &app.theme;
    let rows = heatmap_rows(&app.heatmap_language);
    let row_height = 3;
    let row_gap = 0;
    let keyboard_height = rows
        .len()
        .saturating_mul(row_height as usize)
        .saturating_add(
            rows.len()
                .saturating_sub(1)
                .saturating_mul(row_gap as usize),
        ) as u16;
    let keyboard_area = centered(area, area.width, keyboard_height.min(area.height));

    let mut y = keyboard_area.y;
    for row in rows {
        if y.saturating_add(row_height) > keyboard_area.y.saturating_add(keyboard_area.height) {
            break;
        }
        let row_width = heatmap_row_width(&row);
        let mut x = keyboard_area
            .x
            .saturating_add(keyboard_area.width.saturating_sub(row_width) / 2)
            .saturating_add(row.offset);
        for key in row.keys {
            if x >= keyboard_area.x.saturating_add(keyboard_area.width) {
                break;
            }
            let rect = Rect {
                x,
                y,
                width: key.width.min(
                    keyboard_area
                        .x
                        .saturating_add(keyboard_area.width)
                        .saturating_sub(x),
                ),
                height: row_height,
            };
            if rect.width == 0 {
                break;
            }
            let errors = counts.get(key.key).copied().unwrap_or(0);
            let hovered = app.heatmap_hover_key.as_deref() == Some(key.key);
            let color = heatmap_key_color(errors, max_errors, theme);
            let border_color = if hovered { theme.text } else { color };
            let border_style = Style::default().fg(border_color);
            let label_style = if hovered {
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(color)
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_set(heatmap_key_border_set(errors, max_errors))
                .border_style(border_style)
                .style(Style::default().bg(theme.background));
            frame.render_widget(
                Paragraph::new(heatmap_key_label_line(
                    key.label,
                    errors,
                    total_errors,
                    hovered,
                    label_style,
                ))
                .block(block)
                .alignment(Alignment::Center)
                .style(Style::default().bg(theme.background)),
                rect,
            );
            x = x.saturating_add(key.width).saturating_add(1);
        }
        y = y.saturating_add(row_height).saturating_add(row_gap);
    }
}

fn heatmap_key_at(language: &str, area: Rect, x: u16, y: u16) -> Option<String> {
    let rows = heatmap_rows(language);
    let row_height = 3;
    let row_gap = 0;
    let keyboard_height = rows
        .len()
        .saturating_mul(row_height as usize)
        .saturating_add(
            rows.len()
                .saturating_sub(1)
                .saturating_mul(row_gap as usize),
        ) as u16;
    let keyboard_area = centered(area, area.width, keyboard_height.min(area.height));

    let mut row_y = keyboard_area.y;
    for row in rows {
        if row_y.saturating_add(row_height) > keyboard_area.y.saturating_add(keyboard_area.height) {
            break;
        }
        let row_width = heatmap_row_width(&row);
        let mut key_x = keyboard_area
            .x
            .saturating_add(keyboard_area.width.saturating_sub(row_width) / 2)
            .saturating_add(row.offset);
        for key in row.keys {
            if key_x >= keyboard_area.x.saturating_add(keyboard_area.width) {
                break;
            }
            let rect = Rect {
                x: key_x,
                y: row_y,
                width: key.width,
                height: row_height,
            };
            if in_rect(x, y, rect) {
                return Some(key.key.to_string());
            }
            key_x = key_x.saturating_add(key.width).saturating_add(1);
        }
        row_y = row_y.saturating_add(row_height).saturating_add(row_gap);
    }
    None
}

fn heatmap_rows(language: &str) -> Vec<HeatmapRowSpec> {
    match language {
        "ru" => vec![
            heatmap_row(
                0,
                &[
                    "ё", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "=",
                ],
            ),
            heatmap_row(
                3,
                &["й", "ц", "у", "к", "е", "н", "г", "ш", "щ", "з", "х", "ъ"],
            ),
            heatmap_row(6, &["ф", "ы", "в", "а", "п", "р", "о", "л", "д", "ж", "э"]),
            heatmap_row(9, &["я", "ч", "с", "м", "и", "т", "ь", "б", "ю", "."]),
            heatmap_row(0, &HEATMAP_RU_EXTRA_SYMBOL_ROW_1),
            heatmap_row(3, &HEATMAP_RU_EXTRA_SYMBOL_ROW_2),
        ],
        _ => vec![
            heatmap_row(
                0,
                &[
                    "`", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "=",
                ],
            ),
            heatmap_row(
                3,
                &["q", "w", "e", "r", "t", "y", "u", "i", "o", "p", "[", "]"],
            ),
            heatmap_row(6, &["a", "s", "d", "f", "g", "h", "j", "k", "l", ";", "'"]),
            heatmap_row(9, &["z", "x", "c", "v", "b", "n", "m", ",", ".", "/"]),
            heatmap_row(0, &HEATMAP_EN_SHIFT_SYMBOL_ROW_1),
            heatmap_row(20, &HEATMAP_EN_SHIFT_SYMBOL_ROW_2),
        ],
    }
}

fn heatmap_row(offset: u16, labels: &[&'static str]) -> HeatmapRowSpec {
    HeatmapRowSpec {
        offset,
        keys: labels
            .iter()
            .map(|label| HeatmapKeySpec {
                key: label,
                label,
                width: 5,
            })
            .collect(),
    }
}

fn heatmap_row_width(row: &HeatmapRowSpec) -> u16 {
    row.offset
        .saturating_add(row.keys.iter().map(|key| key.width).sum::<u16>())
        .saturating_add(row.keys.len().saturating_sub(1) as u16)
}

fn heatmap_display_key(key: &str) -> String {
    if key == "space" {
        "space".to_string()
    } else {
        key.to_string()
    }
}

fn heatmap_key_label_line(
    label: &'static str,
    errors: usize,
    total_errors: usize,
    hovered: bool,
    style: Style,
) -> Line<'static> {
    if hovered {
        return Line::from(Span::styled(
            heatmap_error_percent_label(errors, total_errors),
            style,
        ))
        .centered();
    }

    Line::from(Span::styled(label, style)).centered()
}

fn heatmap_error_percent_label(errors: usize, total_errors: usize) -> String {
    if total_errors == 0 {
        return "0%".to_string();
    }

    let percent = (errors as f64 / total_errors as f64 * 100.0).round() as usize;
    let percent = if errors == 0 { 0 } else { percent.clamp(1, 99) };
    format!("{percent}%")
}

fn heatmap_key_border_set(_errors: usize, _max_errors: usize) -> symbols::border::Set {
    symbols::border::ROUNDED
}

fn heatmap_error_ratio(errors: usize, max_errors: usize) -> f64 {
    if errors == 0 || max_errors == 0 {
        0.0
    } else {
        errors as f64 / max_errors as f64
    }
}

fn heatmap_key_color(
    errors: usize,
    max_errors: usize,
    theme: &crate::themes::ResolvedTheme,
) -> Color {
    if errors == 0 || max_errors == 0 {
        return theme.muted;
    }

    let ratio = heatmap_error_ratio(errors, max_errors);
    heatmap_rgb_gradient_color(ratio).unwrap_or_else(|| heatmap_indexed_color(ratio))
}

fn heatmap_rgb_gradient_color(ratio: f64) -> Option<Color> {
    let ratio = ratio.clamp(0.0, 1.0);
    let last_index = HEATMAP_RGB_PALETTE.len().saturating_sub(1);
    let scaled = ratio * last_index as f64;
    let index = scaled.floor() as usize;
    if index >= last_index {
        return Some(HEATMAP_RGB_PALETTE[last_index]);
    }

    blend_rgb(
        HEATMAP_RGB_PALETTE[index],
        HEATMAP_RGB_PALETTE[index + 1],
        scaled - index as f64,
    )
}

fn heatmap_indexed_color(ratio: f64) -> Color {
    let ratio = ratio.clamp(0.0, 1.0);
    let last_index = HEATMAP_INDEXED_PALETTE.len().saturating_sub(1);
    let index = (ratio * last_index as f64).round() as usize;
    Color::Indexed(HEATMAP_INDEXED_PALETTE[index.min(last_index)])
}

fn blend_rgb(from: Color, to: Color, ratio: f64) -> Option<Color> {
    let Color::Rgb(from_red, from_green, from_blue) = from else {
        return None;
    };
    let Color::Rgb(to_red, to_green, to_blue) = to else {
        return None;
    };

    Some(Color::Rgb(
        blend_channel(from_red, to_red, ratio),
        blend_channel(from_green, to_green, ratio),
        blend_channel(from_blue, to_blue, ratio),
    ))
}

fn blend_channel(from: u8, to: u8, ratio: f64) -> u8 {
    (from as f64 + (to as f64 - from as f64) * ratio)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn compact_date(value: &str) -> String {
    value.replace('T', " ").chars().take(16).collect::<String>()
}

fn row_speed_value(config: &Config, wpm_value: f64) -> f64 {
    match speed_unit(config) {
        "cpm" => wpm_value * 5.0,
        _ => wpm_value,
    }
}

fn speed_threshold_hint(config: &Config) -> &'static str {
    match speed_unit(config) {
        "cpm" => "cpm threshold",
        _ => "wpm threshold",
    }
}

fn render_results(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let metrics = app.session.metrics();
    let content = result_area(app, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        content,
    );

    let top_height = content.height.saturating_sub(11).clamp(10, 18);
    let [top, middle, actions] = Layout::vertical([
        Constraint::Length(top_height),
        Constraint::Length(6),
        Constraint::Length(2),
    ])
    .spacing(1)
    .areas(content);
    let [left, chart_area] = Layout::horizontal([Constraint::Length(14), Constraint::Fill(1)])
        .spacing(1)
        .areas(top);

    let test_type = mode_label_ui(app, &app.session.mode);
    let unit = speed_unit(&app.config);
    let speed_hovered = app.control_hover == Some(ControlHover::ResultSpeed);
    let accuracy_hovered = app.control_hover == Some(ControlHover::ResultAccuracy);
    let test_type_hovered = app.control_hover == Some(ControlHover::ResultTestType);
    let summary = vec![
        Line::from(Span::styled(
            unit,
            hover_style(speed_hovered, speed_hovered, theme.text, theme.muted),
        )),
        Line::from(Span::styled(
            format!("{:.0}", speed_value(&app.config, &metrics)),
            result_value_style(speed_hovered, theme),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            tr(app, "acc", "точн"),
            hover_style(accuracy_hovered, accuracy_hovered, theme.text, theme.muted),
        )),
        Line::from(Span::styled(
            format!("{:.1}%", metrics.accuracy),
            result_value_style(accuracy_hovered, theme),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            tr(app, "test type", "тип теста"),
            hover_style(
                test_type_hovered,
                test_type_hovered,
                theme.text,
                theme.muted,
            ),
        )),
        Line::from(Span::styled(
            test_type,
            result_value_style(test_type_hovered, theme),
        )),
        Line::from(Span::styled(
            app.dictionary.name.clone(),
            result_value_style(test_type_hovered, theme),
        )),
    ];
    frame.render_widget(Paragraph::new(summary), left);

    render_result_chart(
        frame,
        app,
        centered(
            chart_area,
            chart_area.width.saturating_sub(2),
            chart_area.height,
        ),
        speed_value(&app.config, &metrics),
    );

    let stats = Layout::horizontal([
        Constraint::Percentage(22),
        Constraint::Percentage(28),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
    ])
    .spacing(2)
    .split(middle);
    render_result_stat(
        frame,
        stats[0],
        tr(app, "raw", "сырая"),
        format!("{:.0}", raw_speed_value(&app.config, &metrics)),
        app,
    );
    render_result_stat(
        frame,
        stats[1],
        tr(app, "characters", "символы"),
        format!(
            "{}/{}/{}/{}",
            metrics.correct_characters,
            metrics.incorrect_characters,
            metrics.extra_characters,
            metrics.missed_characters
        ),
        app,
    );
    render_result_stat(
        frame,
        stats[2],
        tr(app, "consistency", "стабильность"),
        format!("{:.0}%", metrics.consistency),
        app,
    );
    render_result_stat(
        frame,
        stats[3],
        tr(app, "time", "время"),
        format!("{:.0}s", app.session.elapsed().as_secs_f64()),
        app,
    );

    let action_line = Line::from(result_action_spans(app));
    let hint_line = Line::from(Span::styled(
        result_metric_hint(app, app.control_hover),
        Style::default().fg(theme.muted),
    ));
    frame.render_widget(
        Paragraph::new(vec![action_line, hint_line])
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.muted)),
        actions,
    );
}

fn result_actions_rect(app: &App, area: Rect) -> Rect {
    let content = result_area(app, area);
    let top_height = content.height.saturating_sub(11).clamp(10, 18);
    Layout::vertical([
        Constraint::Length(top_height),
        Constraint::Length(6),
        Constraint::Length(2),
    ])
    .spacing(1)
    .split(content)[2]
}

fn result_action_spans(app: &App) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    push_result_action(
        &mut spans,
        app,
        ControlHover::ResultRestart,
        &app.config.keybindings.restart,
        tr(app, "restart", "рестарт"),
        app.control_hover == Some(ControlHover::ResultRestart),
    );
    spans.push(Span::raw("  "));
    push_result_action(
        &mut spans,
        app,
        ControlHover::ResultRepeat,
        &app.config.keybindings.retry_text,
        tr(app, "repeat", "повтор"),
        app.control_hover == Some(ControlHover::ResultRepeat),
    );
    spans.push(Span::raw("  "));
    push_result_action(
        &mut spans,
        app,
        ControlHover::ResultQuit,
        &app.config.keybindings.close,
        tr(app, "quit", "выход"),
        app.control_hover == Some(ControlHover::ResultQuit),
    );
    spans
}

fn push_result_action(
    spans: &mut Vec<Span<'static>>,
    app: &App,
    control: ControlHover,
    key: &str,
    label: &'static str,
    hovered: bool,
) {
    let theme = &app.theme;
    spans.push(Span::styled(
        key.to_string(),
        Style::default().fg(theme.text),
    ));
    spans.push(Span::raw(" "));
    push_chip(
        spans,
        app,
        control,
        label,
        false,
        hovered,
        (theme.main, theme.muted),
    );
}

fn result_metric_hint(app: &App, hover: Option<ControlHover>) -> &'static str {
    match hover {
        Some(ControlHover::ResultSpeed) => tr(
            app,
            "Final speed based on correct characters, shown in the selected speed unit.",
            "Итоговая скорость по правильным символам в выбранных единицах.",
        ),
        Some(ControlHover::ResultAccuracy) => tr(
            app,
            "Accuracy includes visible errors and corrected mistakes made during the test.",
            "Точность учитывает видимые ошибки и исправления во время теста.",
        ),
        Some(ControlHover::ResultTestType) => tr(
            app,
            "The mode, amount, language, and dictionary used for this test.",
            "Режим, количество, язык и словарь этого теста.",
        ),
        Some(ControlHover::ResultRaw) => tr(
            app,
            "Raw speed counts all typed characters before accuracy penalties.",
            "Сырая скорость считает все набранные символы без штрафов за точность.",
        ),
        Some(ControlHover::ResultCharacters) => tr(
            app,
            "Correct / incorrect / extra / missed character counts.",
            "Количество правильных, неверных, лишних и пропущенных символов.",
        ),
        Some(ControlHover::ResultConsistency) => tr(
            app,
            "How steady your speed was during the test; fewer spikes means higher consistency.",
            "Насколько ровной была скорость; меньше скачков - выше стабильность.",
        ),
        Some(ControlHover::ResultTime) => tr(
            app,
            "Elapsed typing time used for this result.",
            "Время набора, использованное для результата.",
        ),
        _ => tr(
            app,
            "Hover a result metric to see what it means.",
            "Наведите курсор на метрику результата, чтобы увидеть описание.",
        ),
    }
}

fn result_action_label_hit(app: &App, rect: Rect, x: u16, y: u16) -> Option<ControlHover> {
    if y != rect.y || x < rect.x || x >= rect.x.saturating_add(rect.width) {
        return None;
    }

    let items = [
        (
            app.config.keybindings.restart.as_str(),
            tr(app, "restart", "рестарт"),
            Some(ControlHover::ResultRestart),
        ),
        (
            app.config.keybindings.retry_text.as_str(),
            tr(app, "repeat", "повтор"),
            Some(ControlHover::ResultRepeat),
        ),
        (
            app.config.keybindings.close.as_str(),
            tr(app, "quit", "выход"),
            Some(ControlHover::ResultQuit),
        ),
    ];
    let total_width: u16 = items
        .iter()
        .map(|(key, label, _)| key.width() as u16 + 1 + label.width() as u16)
        .sum::<u16>()
        + items.len().saturating_sub(1) as u16 * 2;
    let mut cursor = rect
        .x
        .saturating_add(rect.width.saturating_sub(total_width) / 2);

    for (key, label, action) in items {
        let item_end = cursor
            .saturating_add(key.width() as u16)
            .saturating_add(1)
            .saturating_add(label.width() as u16);
        if x >= cursor && x < item_end {
            return action;
        }
        cursor = item_end.saturating_add(2);
    }

    None
}

fn result_metric_hit(app: &App, area: Rect, x: u16, y: u16) -> Option<ControlHover> {
    let content = result_area(app, area);
    let top_height = content.height.saturating_sub(11).clamp(10, 18);
    let [top, middle, _] = Layout::vertical([
        Constraint::Length(top_height),
        Constraint::Length(6),
        Constraint::Length(2),
    ])
    .spacing(1)
    .areas(content);
    let [left, _] = Layout::horizontal([Constraint::Length(14), Constraint::Fill(1)])
        .spacing(1)
        .areas(top);

    let metrics = app.session.metrics();
    let test_type = mode_label_ui(app, &app.session.mode);
    if line_text_hit(left, 0, speed_unit(&app.config), x, y)
        || line_text_hit(
            left,
            1,
            &format!("{:.0}", speed_value(&app.config, &metrics)),
            x,
            y,
        )
    {
        return Some(ControlHover::ResultSpeed);
    }
    if line_text_hit(left, 3, tr(app, "acc", "точн"), x, y)
        || line_text_hit(left, 4, &format!("{:.1}%", metrics.accuracy), x, y)
    {
        return Some(ControlHover::ResultAccuracy);
    }
    if line_text_hit(left, 6, tr(app, "test type", "тип теста"), x, y)
        || line_text_hit(left, 7, &test_type, x, y)
        || line_text_hit(left, 8, &app.dictionary.name, x, y)
    {
        return Some(ControlHover::ResultTestType);
    }

    let stats = Layout::horizontal([
        Constraint::Percentage(22),
        Constraint::Percentage(28),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
    ])
    .spacing(2)
    .split(middle);
    for (rect, label, value, hover) in [
        (
            stats[0],
            tr(app, "raw", "сырая").to_string(),
            format!("{:.0}", raw_speed_value(&app.config, &metrics)),
            ControlHover::ResultRaw,
        ),
        (
            stats[1],
            tr(app, "characters", "символы").to_string(),
            format!(
                "{}/{}/{}/{}",
                metrics.correct_characters,
                metrics.incorrect_characters,
                metrics.extra_characters,
                metrics.missed_characters
            ),
            ControlHover::ResultCharacters,
        ),
        (
            stats[2],
            tr(app, "consistency", "стабильность").to_string(),
            format!("{:.0}%", metrics.consistency),
            ControlHover::ResultConsistency,
        ),
        (
            stats[3],
            tr(app, "time", "время").to_string(),
            format!("{:.0}s", app.session.elapsed().as_secs_f64()),
            ControlHover::ResultTime,
        ),
    ] {
        if centered_line_text_hit(rect, 0, &label, x, y)
            || centered_line_text_hit(rect, 1, &value, x, y)
        {
            return Some(hover);
        }
    }

    None
}

fn line_text_hit(rect: Rect, row: u16, text: &str, x: u16, y: u16) -> bool {
    if y != rect.y.saturating_add(row) {
        return false;
    }
    let width = text.width() as u16;
    x >= rect.x && x < rect.x.saturating_add(width)
}

fn centered_line_text_hit(rect: Rect, row: u16, text: &str, x: u16, y: u16) -> bool {
    if y != rect.y.saturating_add(row) {
        return false;
    }
    let width = text.width() as u16;
    let start = rect.x.saturating_add(rect.width.saturating_sub(width) / 2);
    x >= start && x < start.saturating_add(width)
}

fn result_area(app: &App, area: Rect) -> Rect {
    let width = area
        .width
        .saturating_sub(4)
        .min(scaled_dim(144, app_ui_scale(app)))
        .max(area.width.min(40));
    let height = area
        .height
        .saturating_sub(4)
        .min(scaled_dim(30, app_ui_scale(app)))
        .max(area.height.min(20));
    transition_rect(app, area, width, height)
}

fn render_result_chart(frame: &mut Frame, app: &App, area: Rect, average_speed: f64) {
    let samples = app.session.samples();
    render_result_chart_from_samples(frame, app, area, &samples, average_speed);
}

fn render_result_chart_from_samples(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    samples: &[SpeedSample],
    average_speed: f64,
) {
    let theme = &app.theme;
    let raw_color = theme.text;
    let avg_color = theme.warning;
    let axis_color = theme.muted;
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        area,
    );
    let multiplier = if speed_unit(&app.config) == "cpm" {
        5.0
    } else {
        1.0
    };
    let speed_data: Vec<(f64, f64)> = samples
        .iter()
        .map(|sample| (sample.second, sample.wpm * multiplier))
        .collect();
    let raw_data: Vec<(f64, f64)> = samples
        .iter()
        .map(|sample| (sample.second, sample.raw_wpm * multiplier))
        .collect();
    let display_speed = moving_average(&speed_data, 4);
    let display_raw = moving_average(&raw_data, 4);
    let x_max = samples
        .last()
        .map(|sample| sample.second.max(1.0))
        .unwrap_or(1.0);
    let y_max = samples
        .iter()
        .fold(100.0_f64, |max, sample| {
            max.max(sample.raw_wpm * multiplier)
                .max(sample.wpm * multiplier)
        })
        .max(average_speed)
        .ceil();
    let average_data = vec![(0.0, average_speed), (x_max, average_speed)];
    let mut previous_errors = 0.0;
    let error_data: Vec<(f64, f64)> = samples
        .iter()
        .filter_map(|sample| {
            let point = (sample.errors > previous_errors).then_some((sample.second, y_max));
            previous_errors = sample.errors;
            point
        })
        .collect();
    let [legend_area, chart_area] =
        Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(area);
    let legend = Line::from(vec![
        Span::styled(
            format!("● {} ", speed_unit(&app.config)),
            Style::default().fg(theme.main).add_modifier(Modifier::BOLD),
        ),
        Span::styled("● raw ", Style::default().fg(raw_color)),
        Span::styled("● avg ", Style::default().fg(avg_color)),
        Span::styled("✕ errors", Style::default().fg(theme.error)),
    ]);
    frame.render_widget(
        Paragraph::new(legend)
            .alignment(Alignment::Right)
            .style(Style::default().fg(theme.muted).bg(theme.background)),
        legend_area,
    );

    let datasets = vec![
        Dataset::default()
            .name("raw")
            .graph_type(GraphType::Line)
            .style(Style::default().fg(raw_color))
            .marker(symbols::Marker::Braille)
            .data(&display_raw),
        Dataset::default()
            .name("avg")
            .graph_type(GraphType::Line)
            .style(Style::default().fg(avg_color))
            .marker(symbols::Marker::Dot)
            .data(&average_data),
        Dataset::default()
            .name(speed_unit(&app.config))
            .graph_type(GraphType::Line)
            .style(Style::default().fg(theme.main))
            .marker(symbols::Marker::Braille)
            .data(&display_speed),
        Dataset::default()
            .name("errors")
            .graph_type(GraphType::Scatter)
            .style(Style::default().fg(theme.error))
            .marker(symbols::Marker::Block)
            .data(&error_data),
    ];
    let chart = Chart::new(datasets)
        .style(Style::default().bg(theme.background))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(pulse_border_style(app, axis_color))
                .style(Style::default().bg(theme.background))
                .title(Span::styled(
                    " performance ",
                    Style::default().fg(axis_color),
                )),
        )
        .x_axis(
            Axis::default()
                .title(Span::styled("seconds", Style::default().fg(axis_color)))
                .style(Style::default().fg(axis_color))
                .bounds([0.0, x_max])
                .labels(vec![
                    Span::styled("0", Style::default().fg(axis_color)),
                    Span::styled(
                        format!("{:.0}", x_max / 2.0),
                        Style::default().fg(axis_color),
                    ),
                    Span::styled(format!("{x_max:.0}"), Style::default().fg(axis_color)),
                ]),
        )
        .y_axis(
            Axis::default()
                .title(Span::styled(
                    match speed_unit(&app.config) {
                        "cpm" => "characters per minute",
                        _ => "words per minute",
                    },
                    Style::default().fg(axis_color),
                ))
                .style(Style::default().fg(axis_color))
                .bounds([0.0, y_max])
                .labels(vec![
                    Span::styled("0", Style::default().fg(axis_color)),
                    Span::styled(
                        format!("{:.0}", y_max / 2.0),
                        Style::default().fg(axis_color),
                    ),
                    Span::styled(format!("{y_max:.0}"), Style::default().fg(axis_color)),
                ]),
        );
    frame.render_widget(chart, chart_area);
    render_border_runner(frame, app, chart_area, Some(" performance "));
}

fn moving_average(data: &[(f64, f64)], window: usize) -> Vec<(f64, f64)> {
    if data.len() <= 2 {
        return data.to_vec();
    }
    let window = window.max(1);
    data.iter()
        .enumerate()
        .map(|(idx, (x, _))| {
            let start = idx.saturating_sub(window - 1);
            let slice = &data[start..=idx];
            let avg = slice.iter().map(|(_, y)| *y).sum::<f64>() / slice.len() as f64;
            (*x, avg)
        })
        .collect()
}

fn render_result_stat(frame: &mut Frame, area: Rect, label: &str, value: String, app: &App) {
    let theme = &app.theme;
    let hovered = result_stat_hover(label).is_some_and(|hover| app.control_hover == Some(hover));
    let lines = vec![
        Line::from(Span::styled(
            label.to_string(),
            hover_style(hovered, hovered, theme.text, theme.muted),
        )),
        Line::from(Span::styled(value, result_value_style(hovered, theme))),
    ];
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
}

fn result_stat_hover(label: &str) -> Option<ControlHover> {
    match label {
        "raw" | "сырая" => Some(ControlHover::ResultRaw),
        "characters" | "символы" => Some(ControlHover::ResultCharacters),
        "consistency" | "стабильность" => Some(ControlHover::ResultConsistency),
        "time" | "время" => Some(ControlHover::ResultTime),
        _ => None,
    }
}

fn result_value_style(hovered: bool, theme: &crate::themes::ResolvedTheme) -> Style {
    Style::default()
        .fg(if hovered { theme.text } else { theme.main })
        .add_modifier(Modifier::BOLD)
}

fn modal(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    title: &str,
    lines: Vec<Line<'_>>,
    width: u16,
    height: u16,
) {
    let theme = &app.theme;
    let rect = transition_rect(app, area, width, height);
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(pulse_border_style(app, theme.main))
        .style(Style::default().fg(theme.text).bg(theme.background));
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Left)
            .style(Style::default().fg(theme.text)),
        rect,
    );
    render_border_runner(frame, app, rect, Some(title));
}

fn settings_modal(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    lines: Vec<Line<'_>>,
    width: u16,
    height: u16,
    description: &'static str,
) {
    let theme = &app.theme;
    let rect = transition_rect(app, area, width, height);
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .title(Span::styled(
            tr(app, "settings", "настройки"),
            Style::default().fg(theme.main).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(pulse_border_style(app, theme.main))
        .style(Style::default().fg(theme.text).bg(theme.background));
    frame.render_widget(block, rect);
    render_border_runner(frame, app, rect, Some(tr(app, "settings", "настройки")));

    let inner = Rect {
        x: rect.x.saturating_add(2),
        y: rect.y.saturating_add(1),
        width: rect.width.saturating_sub(4),
        height: rect.height.saturating_sub(2),
    };
    let table_width = scaled_dim(72, app_ui_scale(app)).min(inner.width);
    let table_height = lines.len() as u16;
    let [top_margin, table, description_area, buttons] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(table_height),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    let _ = top_margin;
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.text).bg(theme.background)),
        centered(table, table_width, table.height),
    );
    frame.render_widget(
        Paragraph::new(description)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(theme.muted).bg(theme.background)),
        centered(
            description_area,
            description_area.width.saturating_sub(2),
            description_area.height,
        ),
    );
    let [history_button, heatmap_button, close_button] = Layout::horizontal([
        Constraint::Percentage(33),
        Constraint::Percentage(34),
        Constraint::Percentage(33),
    ])
    .areas(buttons);
    let history_hovered = app.settings_button_hover == Some(SettingsButtonHover::History);
    let heatmap_hovered = app.settings_button_hover == Some(SettingsButtonHover::Heatmap);
    let close_hovered = app.settings_button_hover == Some(SettingsButtonHover::Close);
    let history_label = format!(
        "{} {}",
        app.config.keybindings.history,
        tr(app, "history", "история")
    );
    let heatmap_label = format!(
        "{} {}",
        app.config.keybindings.heatmap,
        tr(app, "heatmap", "карта")
    );
    let close_label = format!(
        "{} {}",
        app.config.keybindings.close,
        tr(app, "close", "закрыть")
    );
    frame.render_widget(
        Paragraph::new(Line::from(chip(
            app,
            ControlHover::SettingsHistory,
            &history_label,
            false,
            history_hovered,
            theme.main,
            theme.muted,
        )))
        .alignment(Alignment::Center)
        .style(Style::default().bg(theme.background)),
        history_button,
    );
    frame.render_widget(
        Paragraph::new(Line::from(chip(
            app,
            ControlHover::SettingsHeatmap,
            &heatmap_label,
            false,
            heatmap_hovered,
            theme.main,
            theme.muted,
        )))
        .alignment(Alignment::Center)
        .style(Style::default().bg(theme.background)),
        heatmap_button,
    );
    frame.render_widget(
        Paragraph::new(Line::from(chip(
            app,
            ControlHover::SettingsClose,
            &close_label,
            false,
            close_hovered,
            theme.main,
            theme.muted,
        )))
        .alignment(Alignment::Center)
        .style(Style::default().bg(theme.background)),
        close_button,
    );
}

#[derive(Clone)]
struct WordToken {
    spans: Vec<Span<'static>>,
    width: u16,
    start: usize,
    end: usize,
    separator: Option<(usize, CharState)>,
}

fn wrap_styled_words(app: &App, max_width: u16, visible_lines: usize) -> Vec<Line<'static>> {
    let theme = &app.theme;
    let tokens = word_tokens(app);
    if tokens.is_empty() {
        return vec![
            Line::from(Span::styled(
                tr(app, "start typing", "начните ввод"),
                Style::default().fg(theme.muted),
            ))
            .centered(),
        ];
    }

    let mut packed: Vec<Vec<WordToken>> = Vec::new();
    let mut current = Vec::new();
    let mut line_width = 0;
    let max_width = max_width.max(20);

    for token in tokens {
        let required = token.width.saturating_add(separator_width(&token));
        if !current.is_empty() && line_width + required > max_width {
            packed.push(current);
            current = Vec::new();
            line_width = 0;
        }
        line_width += required;
        current.push(token);
    }
    if !current.is_empty() {
        packed.push(current);
    }

    let cursor = app.session.input.len();
    let active_line = packed
        .iter()
        .position(|line| {
            line.iter().any(|token| {
                cursor >= token.start
                    && (cursor <= token.end
                        || token.separator.is_some_and(|(idx, _)| cursor <= idx))
            })
        })
        .unwrap_or(0);
    let start = active_line.saturating_sub(1);

    let mut lines = Vec::new();
    for line_tokens in packed.into_iter().skip(start).take(visible_lines) {
        let mut spans = Vec::new();
        for token in line_tokens {
            spans.extend(token.spans);
            if let Some((space_idx, state)) = token.separator {
                spans.push(styled_char(app, ' ', state, space_idx));
            }
        }
        lines.push(Line::from(spans).centered());
    }

    while lines.len() < visible_lines {
        lines.push(Line::raw(""));
    }

    lines
}

fn word_tokens(app: &App) -> Vec<WordToken> {
    let states = app.session.char_states();
    let mut tokens = Vec::new();
    let mut spans = Vec::new();
    let mut width = 0;
    let mut start = 0;
    let mut in_word = false;

    for (idx, (ch, state)) in states.iter().copied().enumerate() {
        if ch.is_whitespace() {
            if in_word {
                tokens.push(WordToken {
                    spans,
                    width,
                    start,
                    end: idx.saturating_sub(1),
                    separator: Some((idx, state)),
                });
                spans = Vec::new();
                width = 0;
                in_word = false;
            }
            continue;
        }

        if !in_word {
            start = idx;
            in_word = true;
        }
        width += char_render_width(ch);
        spans.push(styled_char(app, ch, state, idx));
    }

    if in_word {
        tokens.push(WordToken {
            spans,
            width,
            start,
            end: states.len().saturating_sub(1),
            separator: None,
        });
    }

    tokens
}

fn separator_width(token: &WordToken) -> u16 {
    u16::from(token.separator.is_some())
}

fn char_render_width(ch: char) -> u16 {
    ch.width().unwrap_or(1) as u16
}

fn styled_char(app: &App, ch: char, state: CharState, idx: usize) -> Span<'static> {
    let theme = &app.theme;
    let color = if app.config.blind_mode {
        match state {
            CharState::Pending | CharState::Missed => theme.muted,
            CharState::Correct | CharState::Incorrect | CharState::Extra => theme.text,
        }
    } else {
        match state {
            CharState::Pending => theme.muted,
            CharState::Correct => theme.text,
            CharState::Incorrect => theme.error,
            CharState::Extra => theme.warning,
            CharState::Missed => theme.error,
        }
    };
    if idx != app.session.input.len() {
        return Span::styled(ch.to_string(), Style::default().fg(color));
    }

    match app.config.cursor_style.as_str() {
        "underline" => Span::styled(
            ch.to_string(),
            Style::default()
                .fg(theme.caret)
                .add_modifier(Modifier::UNDERLINED),
        ),
        "color" => Span::styled(ch.to_string(), Style::default().fg(theme.caret)),
        _ => Span::styled(ch.to_string(), Style::default().fg(color).bg(theme.caret)),
    }
}

fn with_line_spacing(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    let mut spaced = Vec::with_capacity(lines.len() * 2);
    let last = lines.len().saturating_sub(1);
    for (idx, line) in lines.into_iter().enumerate() {
        spaced.push(line);
        if idx < last {
            spaced.push(Line::raw(""));
        }
    }
    spaced
}

fn panel(line: Line<'static>, theme: &crate::themes::ResolvedTheme) -> Paragraph<'static> {
    Paragraph::new(line)
        .alignment(Alignment::Center)
        .style(Style::default().fg(theme.muted).bg(theme.background))
        .block(Block::default().style(Style::default().bg(theme.background)))
}

fn setting_value_ui(app: &App, value: &str) -> String {
    if !ui_ru(app) {
        return value.to_string();
    }
    match value {
        "on" => "вкл".to_string(),
        "off" => "выкл".to_string(),
        "typing" => "набор".to_string(),
        "flex" => "гибко".to_string(),
        _ => value.to_string(),
    }
}

fn setting_value_row(
    label: &'static str,
    value: &str,
    hint: &'static str,
    theme: &crate::themes::ResolvedTheme,
    active: bool,
) -> Line<'static> {
    let control = setting_control(value);
    let label_style = if active {
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.muted)
    };
    Line::from(vec![
        Span::styled(format!("{label:<SETTINGS_LABEL_WIDTH$}"), label_style),
        Span::styled(
            format!("{control:^SETTINGS_CONTROL_WIDTH$}"),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{hint:<SETTINGS_HINT_WIDTH$}"),
            Style::default().fg(theme.muted),
        ),
    ])
}

fn setting_toggle_row(
    label: &'static str,
    value: bool,
    hint: &'static str,
    theme: &crate::themes::ResolvedTheme,
    active: bool,
    config: &Config,
) -> Line<'static> {
    let switch = setting_control(on_off_for_config(config, value));
    let label_style = if active {
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.muted)
    };
    Line::from(vec![
        Span::styled(format!("{label:<SETTINGS_LABEL_WIDTH$}"), label_style),
        Span::styled(
            format!("{switch:^SETTINGS_CONTROL_WIDTH$}"),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{hint:<SETTINGS_HINT_WIDTH$}"),
            Style::default().fg(theme.muted),
        ),
    ])
}

fn setting_control(value: &str) -> String {
    if matches!(value, "on" | "вкл") {
        let width = value.width();
        let left = SETTINGS_VALUE_WIDTH.saturating_sub(width) / 2;
        let left = left.saturating_sub(1);
        let right = SETTINGS_VALUE_WIDTH.saturating_sub(width + left);
        return format!("< {}{}{} >", " ".repeat(left), value, " ".repeat(right));
    }
    format!("< {value:^SETTINGS_VALUE_WIDTH$} >")
}

fn chip(
    app: &App,
    control: ControlHover,
    label: &str,
    active: bool,
    hovered: bool,
    main: Color,
    muted: Color,
) -> Vec<Span<'static>> {
    let bold_cutoff = activation_bold_cutoff(app, control, label, active);
    let hover_cutoff = hover_color_cutoff(app, control, label, hovered);
    label
        .chars()
        .enumerate()
        .map(|(idx, ch)| {
            let color = if active || hover_cutoff.is_some_and(|cutoff| idx < cutoff) {
                main
            } else {
                muted
            };
            let mut style = Style::default().fg(color);
            if bold_cutoff.is_some_and(|cutoff| idx < cutoff) {
                style = style.add_modifier(Modifier::BOLD);
            }
            Span::styled(ch.to_string(), style)
        })
        .collect()
}

fn hover_color_cutoff(
    app: &App,
    control: ControlHover,
    label: &str,
    hovered: bool,
) -> Option<usize> {
    let Some(animation) = hover_animation(app, control) else {
        return hovered.then_some(label.chars().count());
    };
    let duration = 150.0;
    let elapsed = animation.started_at.elapsed().as_millis() as f64;
    if elapsed >= duration {
        return hovered.then_some(label.chars().count());
    }
    let len = label.chars().count().max(1);
    match animation.kind {
        ControlAnimationKind::HoverIn => Some(((elapsed / duration) * len as f64).ceil() as usize),
        ControlAnimationKind::HoverOut => {
            Some(len.saturating_sub(((elapsed / duration) * len as f64).ceil() as usize))
        }
        _ => hovered.then_some(len),
    }
}

fn hover_animation(app: &App, control: ControlHover) -> Option<crate::app::ControlAnimation> {
    app.control_animation
        .filter(|animation| {
            animation.target == control
                && matches!(
                    animation.kind,
                    ControlAnimationKind::HoverIn | ControlAnimationKind::HoverOut
                )
        })
        .or_else(|| {
            app.hover_exit_animation
                .filter(|animation| animation.target == control)
        })
}

fn activation_bold_cutoff(
    app: &App,
    control: ControlHover,
    label: &str,
    active: bool,
) -> Option<usize> {
    let Some(animation) = app.control_animation else {
        return active.then_some(label.chars().count());
    };
    if animation.target != control {
        return active.then_some(label.chars().count());
    }
    let duration = 180.0;
    let elapsed = animation.started_at.elapsed().as_millis() as f64;
    if elapsed >= duration {
        return active.then_some(label.chars().count());
    }
    let len = label.chars().count().max(1);
    match animation.kind {
        ControlAnimationKind::ActivateIn => {
            Some(((elapsed / duration) * len as f64).ceil() as usize)
        }
        ControlAnimationKind::ActivateOut => {
            Some(len.saturating_sub(((elapsed / duration) * len as f64).ceil() as usize))
        }
        _ => active.then_some(len),
    }
}

fn hover_style(
    hovered: bool,
    active: bool,
    main: ratatui::style::Color,
    muted: ratatui::style::Color,
) -> Style {
    let mut style = Style::default().fg(if active || hovered { main } else { muted });
    if active {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

fn app_ui_scale(_app: &App) -> u16 {
    FIXED_UI_SCALE
}

fn scaled_dim(base: u16, scale: u16) -> u16 {
    ((base as u32 * scale as u32 + 50) / 100).max(1) as u16
}

fn proportional_rects(area: Rect, preferred: [u16; 5], spacing: u16) -> [Rect; 5] {
    let total_spacing = spacing.saturating_mul(4).min(area.width);
    let available = area.width.saturating_sub(total_spacing);
    let preferred_sum = preferred.iter().copied().sum::<u16>().max(1);
    let mut widths = preferred;

    if preferred_sum > available && available > 0 {
        let mut used = 0u16;
        for (idx, width) in widths.iter_mut().enumerate() {
            if idx == preferred.len() - 1 {
                *width = available.saturating_sub(used).max(1);
            } else {
                *width = ((*width as u32 * available as u32) / preferred_sum as u32)
                    .max(1)
                    .min(available as u32) as u16;
                used = used.saturating_add(*width);
            }
        }
    }

    let mut rects = [Rect::default(); 5];
    let mut x = area.x;
    for (idx, width) in widths.into_iter().enumerate() {
        let remaining = area.x.saturating_add(area.width).saturating_sub(x);
        let width = width.min(remaining);
        rects[idx] = Rect {
            x,
            y: area.y,
            width,
            height: area.height,
        };
        x = x.saturating_add(width).saturating_add(spacing);
    }
    rects
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let horizontal = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(width),
        Constraint::Fill(1),
    ])
    .split(area)[1];
    Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .split(horizontal)[1]
}

fn inset(area: Rect, x: u16, y: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(x),
        y: area.y.saturating_add(y),
        width: area.width.saturating_sub(x * 2),
        height: area.height.saturating_sub(y * 2),
    }
}

fn in_rect(x: u16, y: u16, rect: Rect) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn label_hit(rect: Rect, labels: &[&str], x: u16, y: u16) -> Option<usize> {
    if y != rect.y || x < rect.x || x >= rect.x.saturating_add(rect.width) {
        return None;
    }

    let total_width: u16 = labels.iter().map(|label| label.width() as u16).sum::<u16>()
        + labels.len().saturating_sub(1) as u16 * 2;
    let mut cursor = rect
        .x
        .saturating_add(rect.width.saturating_sub(total_width) / 2);

    for (idx, label) in labels.iter().enumerate() {
        let len = label.width() as u16;
        let start = cursor.saturating_sub(2);
        let end = cursor.saturating_add(len).saturating_add(2);
        if x >= start && x < end {
            return Some(idx);
        }
        cursor = cursor.saturating_add(len).saturating_add(2);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_theme() -> crate::themes::ResolvedTheme {
        crate::themes::ResolvedTheme {
            name: "test".to_string(),
            background: Color::Rgb(0, 0, 0),
            text: Color::Rgb(255, 255, 255),
            muted: Color::Rgb(10, 10, 10),
            main: Color::Rgb(0, 200, 0),
            error: Color::Rgb(200, 0, 0),
            warning: Color::Rgb(200, 200, 0),
            caret: Color::Rgb(0, 200, 0),
        }
    }

    #[test]
    fn heatmap_color_uses_multiple_rgb_gradient_steps() {
        let theme = test_theme();
        let colors = [
            heatmap_key_color(1, 8, &theme),
            heatmap_key_color(2, 8, &theme),
            heatmap_key_color(3, 8, &theme),
            heatmap_key_color(4, 8, &theme),
            heatmap_key_color(5, 8, &theme),
            heatmap_key_color(6, 8, &theme),
            heatmap_key_color(7, 8, &theme),
            heatmap_key_color(8, 8, &theme),
        ];

        assert!(colors.windows(2).all(|window| window[0] != window[1]));
        assert_eq!(
            colors[7],
            HEATMAP_RGB_PALETTE[HEATMAP_RGB_PALETTE.len() - 1]
        );
    }

    #[test]
    fn heatmap_color_uses_vivid_gradient_for_terminal_theme() {
        let mut theme = test_theme();
        theme.main = Color::Yellow;
        theme.warning = Color::Yellow;
        theme.error = Color::Red;

        assert_eq!(heatmap_key_color(0, 8, &theme), theme.muted);
        assert_eq!(heatmap_key_color(8, 8, &theme), HEATMAP_RGB_PALETTE[5]);
    }

    #[test]
    fn heatmap_border_set_keeps_rounded_keys() {
        assert_eq!(heatmap_key_border_set(0, 10), symbols::border::ROUNDED);
        assert_eq!(heatmap_key_border_set(2, 10), symbols::border::ROUNDED);
        assert_eq!(heatmap_key_border_set(4, 10), symbols::border::ROUNDED);
        assert_eq!(heatmap_key_border_set(7, 10), symbols::border::ROUNDED);
    }

    #[test]
    fn heatmap_error_percent_label_is_only_for_hover() {
        assert_eq!(heatmap_error_percent_label(0, 227), "0%");
        assert_eq!(heatmap_error_percent_label(1, 227), "1%");
        assert_eq!(heatmap_error_percent_label(20, 227), "9%");
        assert_eq!(heatmap_error_percent_label(227, 227), "99%");
    }
}
