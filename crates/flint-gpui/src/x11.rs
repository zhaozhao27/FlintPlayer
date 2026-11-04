use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageData, ClientMessageEvent, ConnectionExt, EventMask, PropMode,
};
use x11rb::rust_connection::RustConnection;

pub struct OwnWindow {
    connection: RustConnection,
    root: u32,
    window: u32,
    screen_width: u32,
    screen_height: u32,
}

impl OwnWindow {
    pub fn find() -> Option<Self> {
        let (connection, screen_number) = x11rb::connect(None).ok()?;
        let screen = connection.setup().roots.get(screen_number)?;
        let (root, screen_width, screen_height) = (
            screen.root,
            screen.width_in_pixels as u32,
            screen.height_in_pixels as u32,
        );
        let pid = std::process::id();
        let pid_atom = connection
            .intern_atom(false, b"_NET_WM_PID")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let tree = connection.query_tree(root).ok()?.reply().ok()?;
        for &window in &tree.children {
            let owners = connection
                .get_property(false, window, pid_atom, AtomEnum::CARDINAL, 0, 1)
                .ok()?
                .reply()
                .ok()?
                .value32()
                .map(|values| values.collect::<Vec<u32>>())
                .unwrap_or_default();
            if owners.first() == Some(&pid) {
                return Some(Self {
                    connection,
                    root,
                    window,
                    screen_width,
                    screen_height,
                });
            }
        }
        None
    }

    pub fn screen_size(&self) -> (u32, u32) {
        (self.screen_width, self.screen_height)
    }

    pub fn geometry(&self) -> Option<(i32, i32, u32, u32)> {
        let geometry = self
            .connection
            .get_geometry(self.window)
            .ok()?
            .reply()
            .ok()?;
        let translated = self
            .connection
            .translate_coordinates(self.window, self.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        Some((
            translated.dst_x as i32,
            translated.dst_y as i32,
            geometry.width as u32,
            geometry.height as u32,
        ))
    }

    pub fn move_to(&self, x: i32, y: i32) -> bool {
        self.connection
            .configure_window(
                self.window,
                &x11rb::protocol::xproto::ConfigureWindowAux::new()
                    .x(x)
                    .y(y),
            )
            .is_ok()
            && self.connection.flush().is_ok()
    }

    pub fn set_above(&self, above: bool) -> bool {
        let Ok(state_atom) = self.atom(b"_NET_WM_STATE") else {
            return false;
        };
        let Ok(above_atom) = self.atom(b"_NET_WM_STATE_ABOVE") else {
            return false;
        };
        let event = ClientMessageEvent::new(
            32,
            self.window,
            state_atom,
            ClientMessageData::from([1, above_atom as u32, 0, 1, 0]),
        );
        self.connection
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .is_ok()
            && self
                .connection
                .change_property(
                    PropMode::REPLACE,
                    self.window,
                    state_atom,
                    AtomEnum::ATOM,
                    32,
                    if above { 1 } else { 0 },
                    &if above {
                        above_atom.to_ne_bytes().to_vec()
                    } else {
                        Vec::new()
                    },
                )
                .is_ok()
            && self.connection.flush().is_ok()
    }

    fn atom(&self, name: &[u8]) -> Result<u32, x11rb::errors::ReplyError> {
        Ok(self.connection.intern_atom(false, name)?.reply()?.atom)
    }
}
