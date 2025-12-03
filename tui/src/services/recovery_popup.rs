use crate::services::detect_term::{self, is_unsupported_terminal};
use crate::services::markdown_renderer::render_markdown_to_lines_safe;
use popup_widget::{PopupConfig, PopupPosition, PopupWidget, StyledLineContent, Tab};
use ratatui::layout::Size;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use regex;
use serde::{Deserialize, Serialize};
use stakpak_api::models::RecoveryOption;
use stakpak_shared::models::integrations::openai::{MessageContent, Role};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryOperation {
    Append,
    Truncate,
    RemoveTools,
    RevertToCheckpoint,
    ChangeModel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub model: String,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryAction {
    pub message_index: usize,
    pub role: Option<Role>,
    pub content: Option<MessageContent>,
    pub failed_tool_call_ids_to_remove: Option<Vec<String>>,
    pub recovery_operation: RecoveryOperation,
    pub revert_to_checkpoint: Option<String>,
    pub model_config: Option<ModelConfig>,
    pub explanation: Option<String>,
}

pub struct RecoveryPopupService {
    popup: PopupWidget,
    recovery_options: Vec<RecoveryOption>,
    selected_index: usize,
    terminal_size: ratatui::layout::Rect,
    is_maximized: bool,
    tab_scroll_positions: Vec<Option<usize>>,
}

impl Default for RecoveryPopupService {
    fn default() -> Self {
        Self::new()
    }
}

impl RecoveryPopupService {
    pub fn new() -> Self {
        Self {
            popup: Self::create_empty_popup(),
            recovery_options: Vec::new(),
            selected_index: 0,
            terminal_size: ratatui::layout::Rect::new(0, 0, 80, 24),
            is_maximized: false,
            tab_scroll_positions: Vec::new(),
        }
    }

    pub fn new_with_recovery_options(
        recovery_options: Vec<RecoveryOption>,
        terminal_size: Size,
    ) -> Self {
        let term_rect = ratatui::layout::Rect::new(0, 0, terminal_size.width, terminal_size.height);

        let mut service = Self {
            popup: Self::create_empty_popup(),
            recovery_options: recovery_options.clone(),
            selected_index: 0,
            terminal_size: term_rect,
            is_maximized: false,
            tab_scroll_positions: vec![None; recovery_options.len()],
        };

        service.popup = service.create_popup_with_options(&recovery_options, term_rect);
        // Don't auto-show - visibility should be controlled by the caller

        service
    }

    pub fn is_visible(&self) -> bool {
        self.popup.is_visible()
    }

    pub fn render(&mut self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        // Ensure popup is shown before rendering
        if !self.popup.is_visible() {
            self.popup.show();
        }
        self.popup.render(f, area);
    }

    pub fn show(&mut self) {
        self.popup.show();
    }

    pub fn scroll_up(&mut self) {
        // Scrolling disabled as per user request
        // let _ = self.popup.handle_event(popup_widget::PopupEvent::ScrollUp);
    }

    pub fn scroll_down(&mut self) {
        // Scrolling disabled as per user request
        // let _ = self.popup.handle_event(popup_widget::PopupEvent::ScrollDown);
    }

    pub fn prev_tab(&mut self) {
        self.save_current_scroll_position();
        let _ = self.popup.handle_event(popup_widget::PopupEvent::PrevTab);
        self.selected_index = self.popup.state().selected_tab;
        self.restore_scroll_position();
    }

    pub fn next_tab(&mut self) {
        self.save_current_scroll_position();
        let _ = self.popup.handle_event(popup_widget::PopupEvent::NextTab);
        self.selected_index = self.popup.state().selected_tab;
        self.restore_scroll_position();
    }

    pub fn escape(&mut self) {
        let _ = self.popup.handle_event(popup_widget::PopupEvent::Escape);
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn set_selected_index(&mut self, index: usize) {
        if index < self.recovery_options.len() {
            self.selected_index = index;
            self.popup.set_selected_tab(index);
        }
    }

    pub fn selected_option(&self) -> Option<&RecoveryOption> {
        self.recovery_options.get(self.selected_index)
    }

    pub fn recreate_with_terminal_size(&mut self, new_terminal_size: Size) {
        if self.recovery_options.is_empty() {
            return;
        }

        let was_visible = self.is_visible();
        let current_selected_index = self.selected_index;
        let current_options = self.recovery_options.clone();
        let current_scroll_positions = self.tab_scroll_positions.clone();

        let term_rect =
            ratatui::layout::Rect::new(0, 0, new_terminal_size.width, new_terminal_size.height);
        self.terminal_size = term_rect;
        self.popup = self.create_popup_with_options(&current_options, term_rect);

        self.popup.set_selected_tab(current_selected_index);
        self.tab_scroll_positions = current_scroll_positions;

        if was_visible {
            self.popup.show();
            self.restore_scroll_position();
        }
    }

    pub fn toggle_maximize(&mut self) {
        self.is_maximized = !self.is_maximized;

        if !self.recovery_options.is_empty() {
            let was_visible = self.is_visible();
            let current_selected_index = self.selected_index;
            let current_options = self.recovery_options.clone();
            let current_scroll_positions = self.tab_scroll_positions.clone();

            self.popup = self.create_popup_with_options(&current_options, self.terminal_size);
            self.popup.set_selected_tab(current_selected_index);
            self.tab_scroll_positions = current_scroll_positions;

            if was_visible {
                self.popup.show();
                self.restore_scroll_position();
            }
        }
    }

    pub fn is_maximized(&self) -> bool {
        self.is_maximized
    }

    fn calculate_dynamic_popup_size(
        &self,
        recovery_options: &[RecoveryOption],
        terminal_size: ratatui::layout::Rect,
    ) -> (u16, u16) {
        const MIN_HEIGHT: u16 = 15;
        const MAX_HEIGHT_PERCENT: f32 = 0.6;
        const WIDTH_PERCENT: f32 = 1.0; // 100% width

        const BORDER_HEIGHT: usize = 2;
        const TITLE_HEIGHT: usize = 2;
        const TAB_HEADER_HEIGHT: usize = 2;
        const FOOTER_HEIGHT: usize = 1;
        const SUBHEADER_HEIGHT: usize = 2;
        const SPACING_BUFFER: usize = 2;

        let total_ui_overhead = BORDER_HEIGHT
            + TITLE_HEIGHT
            + TAB_HEADER_HEIGHT
            + FOOTER_HEIGHT
            + SUBHEADER_HEIGHT
            + SPACING_BUFFER;

        let safe_terminal_height = terminal_size.height.max(32);
        let max_popup_height_lines = (safe_terminal_height as f32 * MAX_HEIGHT_PERCENT) as usize;
        let max_content_lines = max_popup_height_lines.saturating_sub(total_ui_overhead);

        let mut max_content_height = 0;
        for option in recovery_options.iter() {
            let content = self.create_option_content(option);
            let content_height = content.lines.len() + 2;
            max_content_height = max_content_height.max(content_height);
        }

        let optimal_content_height = if max_content_height <= max_content_lines {
            max_content_height
        } else {
            max_content_lines
        };

        let required_popup_height = optimal_content_height + total_ui_overhead;
        let height = (required_popup_height as u16)
            .max(MIN_HEIGHT)
            .min(safe_terminal_height);
        let width = (terminal_size.width as f32 * WIDTH_PERCENT) as u16;

        (width, height)
    }

    fn calculate_bottom_position(
        &self,
        terminal_size: ratatui::layout::Rect,
    ) -> (u16, u16, u16, u16) {
        let (width, height) = if self.recovery_options.is_empty() {
            (terminal_size.width, 15)
        } else {
            self.calculate_dynamic_popup_size(&self.recovery_options, terminal_size)
        };

        let x = 0; // Full width, start at x=0
        let y = terminal_size
            .y
            .saturating_add(terminal_size.height.saturating_sub(height));

        (width, height, x, y)
    }

    fn create_popup_with_options(
        &self,
        recovery_options: &[RecoveryOption],
        terminal_size: ratatui::layout::Rect,
    ) -> PopupWidget {
        if recovery_options.is_empty() {
            return Self::create_empty_popup();
        }

        let subheaders: Vec<Vec<(Line<'static>, Style)>> = recovery_options
            .iter()
            .map(|option| self.render_subheader(option))
            .collect();

        let (width, height, x, y) = if self.is_maximized {
            (
                terminal_size.width,
                terminal_size.height,
                terminal_size.x,
                terminal_size.y,
            )
        } else {
            self.calculate_bottom_position(terminal_size)
        };

        let tabs: Vec<Tab> = recovery_options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let mode_name = self.format_mode(&option.mode);
                // Add padding left and right to the mode name
                let padded_mode_name = format!("  {}  ", mode_name);
                let styled_title = Line::from(padded_mode_name.clone());

                let content = self.create_option_content(option);
                let subheader = subheaders.get(index).cloned();

                Tab::new_with_custom_title_and_subheader(
                    format!("recovery_option_{}", index),
                    padded_mode_name.clone(),
                    TabContent::new(
                        padded_mode_name.clone(),
                        format!("recovery_option_{}", index),
                        content,
                    ),
                    styled_title,
                    subheader,
                )
            })
            .collect();

        let mut config = PopupConfig::new()
            .title("Recovery Options")
            .title_alignment(popup_widget::Alignment::Left)
            .title_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .background_style(Style::default().bg(Color::Reset))
            .popup_background_style(Style::default().bg(Color::Reset))
            .show_tabs(true)
            .tab_alignment(popup_widget::Alignment::Left)
            .tab_style(Style::default().fg(Color::White).bg(Color::Indexed(235)))
            .selected_tab_style(Style::default().fg(Color::Black).bg(Color::Cyan))
            .tab_borders(false)
            .use_fallback_colors(true)
            .terminal_detector(|| {
                let terminal_info = detect_term::detect_terminal();
                is_unsupported_terminal(&terminal_info.emulator)
            })
            .styled_footer(Some(vec![Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Green)),
                Span::styled(" select  ", Style::default().fg(Color::Indexed(254))),
                Span::styled("←→", Style::default().fg(Color::Yellow)),
                Span::styled(" navigate  ", Style::default().fg(Color::Indexed(254))),
                Span::styled("Ctrl+T", Style::default().fg(Color::Blue)),
                Span::styled(" max/min  ", Style::default().fg(Color::Indexed(254))),
                // Removed scroll hints as per request
                Span::styled("Esc", Style::default().fg(Color::Red)),
                Span::styled(" exit", Style::default().fg(Color::Indexed(254))),
            ])]))
            .footer_style(Some(Style::default().fg(Color::Gray)))
            .position(PopupPosition::Absolute {
                x,
                y,
                width,
                height,
            })
            .text_between_tabs(Some("   ".to_string())) // Increased spacing between tabs
            .text_between_tabs_style(Style::default().fg(Color::Gray));

        for tab in tabs {
            config = config.add_tab(tab);
        }

        PopupWidget::new(config)
    }

    fn create_option_content(&self, option: &RecoveryOption) -> StyledLineContent {
        let mut lines = Vec::new();

        let mut full_content = String::new();

        // Parse state_edits and generate detailed steps
        if let Ok(actions) =
            serde_json::from_value::<Vec<RecoveryAction>>(option.state_edits.clone())
            && !actions.is_empty()
        {
            full_content.push_str("## Steps\n");
            for action in actions {
                let (operation_name, default_explanation) = match action.recovery_operation {
                    RecoveryOperation::Truncate => (
                        "Truncate",
                        format!(
                            "Truncating conversation history after message {}",
                            action.message_index
                        ),
                    ),
                    RecoveryOperation::RemoveTools => {
                        if let Some(ref ids) = action.failed_tool_call_ids_to_remove {
                            let count = ids.len();
                            (
                                "Remove",
                                format!(
                                    "Cleaning up {} action{} in this checkpoint",
                                    count,
                                    if count == 1 { "" } else { "s" }
                                ),
                            )
                        } else {
                            (
                                "Remove",
                                "Cleaning up actions in this checkpoint".to_string(),
                            )
                        }
                    }
                    RecoveryOperation::Append => (
                        "Append",
                        "Adding a message to guide the LLM on what went wrong and how to fix it"
                            .to_string(),
                    ),
                    RecoveryOperation::RevertToCheckpoint => {
                        if let Some(ref ckpt) = action.revert_to_checkpoint {
                            ("Revert", format!("Reverting to checkpoint {}", ckpt))
                        } else {
                            ("Revert", "Reverting to checkpoint".to_string())
                        }
                    }
                    RecoveryOperation::ChangeModel => {
                        if let Some(ref config) = action.model_config {
                            (
                                "Model",
                                format!(
                                    "Switching to {} ({}) for the next 5 turns",
                                    config.model, config.provider
                                ),
                            )
                        } else {
                            (
                                "Model",
                                "Switching to a different model for the next 5 turns".to_string(),
                            )
                        }
                    }
                };

                let explanation = action.explanation.as_ref().unwrap_or(&default_explanation);
                full_content.push_str(&format!("- **{}**: {}\n", operation_name, explanation));
            }

            // Add final step showing checkpoint rollback if available
            if let Some(ref checkpoint_id) = option.revert_to_checkpoint {
                full_content.push_str(&format!(
                    "- Rolling back to checkpoint `{}`\n",
                    checkpoint_id
                ));
            }

            // full_content.push_str("\n# NOTE: These operations are Irreversible!");
        }

        let rendered_markdown = render_markdown_to_lines_safe(&full_content).unwrap_or_default();

        for line in rendered_markdown {
            let mut spans: Vec<Span> = Vec::new();
            // Indentation removed as per request
            spans.extend(line.spans.into_iter().map(|mut span| {
                let has_code_background = span.style.bg.is_some();
                let has_non_gray_fg = span.style.fg.is_some()
                    && span.style.fg != Some(Color::Gray)
                    && span.style.fg != Some(Color::Reset)
                    && span.style.fg != Some(Color::Rgb(220, 220, 220))
                    && span.style.fg != Some(Color::Rgb(180, 180, 180));

                let is_code_span = has_code_background || has_non_gray_fg;

                if is_code_span {
                    span.style = Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD);
                } else {
                    span.style = span.style.fg(Color::Gray);
                }
                span
            }));
            lines.push((Line::from(spans), Style::default()));
        }

        let is_unsupported =
            detect_term::is_unsupported_terminal(&detect_term::detect_terminal().emulator);
        StyledLineContent::new_with_terminal_detection(lines, is_unsupported)
    }

    fn create_empty_popup() -> PopupWidget {
        let config = PopupConfig::new()
            .title("Recovery Options")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .title_alignment(popup_widget::Alignment::Left)
            .border_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .background_style(Style::default().bg(Color::Rgb(25, 26, 38)))
            .popup_background_style(Style::default().bg(Color::Rgb(25, 26, 38)))
            .show_tabs(false)
            .use_fallback_colors(true)
            .text_between_tabs(Some(" ".to_string()))
            .text_between_tabs_style(Style::default().fg(Color::Gray))
            .terminal_detector(|| {
                let terminal_info = detect_term::detect_terminal();
                is_unsupported_terminal(&terminal_info.emulator)
            })
            .fixed_header_lines(0)
            .position(PopupPosition::Absolute {
                x: 0,
                y: 0,
                width: 80,
                height: 15,
            });

        PopupWidget::new(config)
    }

    fn save_current_scroll_position(&mut self) {
        if self.selected_index < self.tab_scroll_positions.len() {
            self.tab_scroll_positions[self.selected_index] = Some(self.popup.state().scroll);
        }
    }

    fn restore_scroll_position(&mut self) {
        if self.selected_index < self.tab_scroll_positions.len()
            && let Some(saved_scroll) = self.tab_scroll_positions[self.selected_index]
        {
            self.popup.state_mut().scroll = saved_scroll;
        }
    }

    fn render_subheader(&self, option: &RecoveryOption) -> Vec<(Line<'static>, Style)> {
        let mut lines = Vec::new();

        let summary = self.summarize_option(option);
        let rendered_markdown = render_markdown_to_lines_safe(&summary).unwrap_or_default();

        for line in rendered_markdown {
            let mut spans: Vec<Span> = Vec::new();
            spans.extend(line.spans.into_iter().map(|mut span| {
                let has_code_background = span.style.bg.is_some();
                let has_non_gray_fg = span.style.fg.is_some()
                    && span.style.fg != Some(Color::Gray)
                    && span.style.fg != Some(Color::Reset)
                    && span.style.fg != Some(Color::Rgb(220, 220, 220))
                    && span.style.fg != Some(Color::Rgb(180, 180, 180));

                let is_code_span = has_code_background || has_non_gray_fg;

                if is_code_span {
                    span.style = Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD);
                } else {
                    span.style = span.style.fg(Color::Gray);
                }
                span
            }));
            lines.push((Line::from(spans), Style::default()));
        }

        lines
    }

    fn format_mode(&self, mode: &stakpak_api::models::RecoveryMode) -> String {
        match mode {
            stakpak_api::models::RecoveryMode::Redirection => "REDIRECTION".to_string(),
            stakpak_api::models::RecoveryMode::Revert => "REVERT".to_string(),
            stakpak_api::models::RecoveryMode::ModelChange => "MODELCHANGE".to_string(),
        }
    }

    fn summarize_option(&self, option: &RecoveryOption) -> String {
        let primary = option.reasoning.clone();
        let sanitized = primary.replace('\n', " ").trim().to_string();

        let markdown_formatted = regex::Regex::new(r"\*([^*]+)\*")
            .ok()
            .map(|re| {
                re.replace_all(&sanitized, |caps: &regex::Captures| {
                    format!("`{}`", &caps[1])
                })
                .to_string()
            })
            .unwrap_or_else(|| sanitized.clone());

        if markdown_formatted.len() > 140 {
            format!(
                "{}...",
                markdown_formatted.chars().take(140).collect::<String>()
            )
        } else {
            markdown_formatted
        }
    }
}

#[derive(Debug)]
struct TabContent {
    title: String,
    id: String,
    styled_content: StyledLineContent,
}

impl TabContent {
    fn new(title: String, id: String, styled_content: StyledLineContent) -> Self {
        Self {
            title,
            id,
            styled_content,
        }
    }
}

impl popup_widget::traits::TabContent for TabContent {
    fn title(&self) -> &str {
        &self.title
    }

    fn id(&self) -> &str {
        &self.id
    }
}

impl popup_widget::traits::PopupContent for TabContent {
    fn render(&self, f: &mut ratatui::Frame, area: ratatui::layout::Rect, scroll: usize) {
        self.styled_content.render(f, area, scroll);
    }

    fn height(&self) -> usize {
        self.styled_content.height()
    }

    fn width(&self) -> usize {
        self.styled_content.width()
    }

    fn get_lines(&self) -> Vec<String> {
        self.styled_content.get_lines()
    }

    fn calculate_rendered_height(&self) -> usize {
        self.styled_content.calculate_rendered_height()
    }

    fn clone_box(&self) -> Box<dyn popup_widget::traits::PopupContent + Send + Sync> {
        Box::new(TabContent {
            title: self.title.clone(),
            id: self.id.clone(),
            styled_content: StyledLineContent::new_with_terminal_detection(
                self.styled_content.lines.clone(),
                self.styled_content.is_unsupported_terminal,
            ),
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
