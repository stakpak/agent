use ratatui::layout::Rect;

/// Position and size configuration for popup widgets
#[derive(Debug, Clone)]
pub enum PopupPosition {
    /// Centered popup with specified width and height
    Centered { width: u16, height: u16 },
    /// Absolute position with specified coordinates and size
    Absolute {
        x: u16,
        y: u16,
        width: u16,
        height: u16,
    },
    /// Relative position (percentage of terminal size)
    Relative {
        x_percent: f32,
        y_percent: f32,
        width_percent: f32,
        height_percent: f32,
    },
    /// Responsive popup that resizes with terminal and shows message when too small
    Responsive {
        width_percent: f32,
        height_percent: f32,
        min_width: u16,
        min_height: u16,
    },
}

impl PopupPosition {
    /// Calculate the actual Rect for the popup given the terminal size
    pub fn calculate_rect(&self, terminal_size: Rect) -> Rect {
        match self {
            PopupPosition::Centered { width, height } => {
                let x = (terminal_size.width.saturating_sub(*width)) / 2;
                let y = (terminal_size.height.saturating_sub(*height)) / 2;
                Rect {
                    x,
                    y,
                    width: *width,
                    height: *height,
                }
            }
            PopupPosition::Absolute {
                x,
                y,
                width,
                height,
            } => Rect {
                x: *x,
                y: *y,
                width: *width,
                height: *height,
            },
            PopupPosition::Relative {
                x_percent,
                y_percent,
                width_percent,
                height_percent,
            } => {
                let x = (terminal_size.width as f32 * x_percent) as u16;
                let y = (terminal_size.height as f32 * y_percent) as u16;
                let width = (terminal_size.width as f32 * width_percent) as u16;
                let height = (terminal_size.height as f32 * height_percent) as u16;
                Rect {
                    x,
                    y,
                    width,
                    height,
                }
            }
            PopupPosition::Responsive {
                width_percent,
                height_percent,
                min_width: _min_width,
                min_height: _min_height,
            } => {
                let width = (terminal_size.width as f32 * width_percent) as u16;
                let height = (terminal_size.height as f32 * height_percent) as u16;
                let x = (terminal_size.width.saturating_sub(width)) / 2;
                let y = (terminal_size.height.saturating_sub(height)) / 2;
                Rect {
                    x,
                    y,
                    width,
                    height,
                }
            }
        }
    }

    /// Check if the viewport is too small for the popup content
    pub fn is_viewport_too_small(&self, terminal_size: Rect) -> bool {
        match self {
            PopupPosition::Responsive {
                min_width,
                min_height,
                ..
            } => terminal_size.width < *min_width || terminal_size.height < *min_height,
            _ => false, // Other position types don't have viewport size checks
        }
    }
}
