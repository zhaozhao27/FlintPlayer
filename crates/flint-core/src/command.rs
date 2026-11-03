#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureCommand {
    StartCapture(String),
    Stop,
    Snapshot,
    SetResolution(u32, u32),
}
