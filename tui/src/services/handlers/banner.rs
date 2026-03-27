use crate::app::{InputEvent, OutputEvent};
use crate::{
    AppState,
    services::commands::{CommandContext, execute_command},
};
use tokio::sync::mpsc::Sender;

pub fn handle_banner_mouse_click(
    state: &mut AppState,
    col: u16,
    row: u16,
    input_tx: &Sender<InputEvent>,
    output_tx: &Sender<OutputEvent>,
) {
    if let Some(banner_area) = state.banner_state.area
        && row >= banner_area.y
        && row < banner_area.y + banner_area.height
        && col >= banner_area.x
        && col < banner_area.x + banner_area.width
    {
        let clicked_cmd = state
            .banner_state
            .click_regions
            .iter()
            .find(|(_, rect)| col >= rect.x && col < rect.x + rect.width && row == rect.y)
            .map(|(cmd, _)| cmd.clone());

        if let Some(cmd) = clicked_cmd {
            let ctx = CommandContext {
                state,
                input_tx,
                output_tx,
            };
            if let Err(e) = execute_command(&cmd, ctx) {
                crate::services::helper_block::push_error_message(state, &e, None);
            } else {
                // Clear banner after successful command execution
                state.banner_state.message = None;
                state.banner_state.click_regions.clear();
                state.banner_state.area = None;
            }
        }
    }
}
