//! Navigation state management for onboarding flow

/// Navigation result indicating what action to take
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NavResult<T> {
    /// Continue forward with selected value
    Forward(T),
    /// Go back to previous step
    Back,
    /// Cancel/exit
    Cancel,
}
