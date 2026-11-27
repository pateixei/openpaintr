use std::{
    collections::BTreeMap,
    io::{self, Cursor, Read},
    os::{fd::AsFd, unix::net::UnixStream},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use openpaintr::{Pixel, decode_png_rgba};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    backend::Backend,
    delegate_noop, event_created_child,
    globals::{Global, GlobalListContents, registry_queue_init},
    protocol::{wl_data_device, wl_data_device_manager, wl_data_offer, wl_registry, wl_seat},
};
use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};

use super::{super::selection::Point, MAX_IMAGE_BYTES, files, tr};

const PNG: &str = "image/png";
const URI_LIST: &str = "text/uri-list";
const MAX_URI_LIST_BYTES: u64 = 64 * 1024;

#[derive(Default)]
struct ImageFormats {
    png: AtomicBool,
    uri_list: AtomicBool,
}

pub(super) struct Clipboard {
    state: State,
    queue: EventQueue<State>,
}

struct State {
    manager: wl_data_device_manager::WlDataDeviceManager,
    seats: BTreeMap<u32, Seat>,
}

struct Seat {
    seat: wl_seat::WlSeat,
    device: wl_data_device::WlDataDevice,
    selection: Option<wl_data_offer::WlDataOffer>,
}

impl Clipboard {
    pub(super) fn new(window: &eframe::CreationContext<'_>) -> Option<Self> {
        let RawDisplayHandle::Wayland(display) = window.display_handle().ok()?.as_raw() else {
            return None;
        };
        // A selection offer is sent to the focused client. Use the same connection as the
        // window, with a separate event queue to avoid dispatching any of winit's events.
        // SAFETY: the eframe event loop owns the display for the lifetime of the app.
        #[expect(
            unsafe_code,
            reason = "wrapping the window's Wayland display requires a foreign pointer"
        )]
        let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
        let connection = Connection::from_backend(backend);
        let (globals, queue) = registry_queue_init(&connection).ok()?;
        let handle = queue.handle();
        let mut state = State {
            manager: globals.bind(&handle, 1..=3, ()).ok()?,
            seats: BTreeMap::new(),
        };
        globals.contents().with_list(|list| {
            for global in list {
                state.bind_seat(globals.registry(), global, &handle);
            }
        });
        queue.flush().ok()?;
        Some(Self { state, queue })
    }

    pub(super) fn supersede_selection(&mut self) {
        // The system write may use X11. Do not paste a Wayland offer that arrived
        // before the copy if the compositor does not bridge the two clipboards.
        let _ = self.queue.roundtrip(&mut self.state);
        for seat in self.state.seats.values_mut() {
            seat.set_selection(None);
        }
    }

    pub(super) fn get_image(&mut self) -> io::Result<Option<(Point, Vec<Pixel>)>> {
        self.queue
            .roundtrip(&mut self.state)
            .map_err(io::Error::other)?;
        let Some((offer, formats)) = self
            .state
            .seats
            .values()
            .filter_map(|seat| seat.selection.as_ref())
            .filter_map(|offer| Some((offer, offer.data::<ImageFormats>()?)))
            .find(|(_, formats)| {
                formats.png.load(Ordering::Relaxed) || formats.uri_list.load(Ordering::Relaxed)
            })
        else {
            return Ok(None);
        };

        if formats.png.load(Ordering::Relaxed) {
            let bytes = self.receive(offer, PNG, MAX_IMAGE_BYTES)?;
            if !bytes.is_empty() {
                let decoded = decode_png_rgba(Cursor::new(&bytes)).map_err(|error| {
                    if error.kind() == io::ErrorKind::OutOfMemory {
                        error
                    } else {
                        io::Error::new(io::ErrorKind::InvalidData, tr("clipboard-image-invalid"))
                    }
                })?;
                return Ok(Some(((decoded.width, decoded.height), decoded.pixels)));
            }
        }
        if formats.uri_list.load(Ordering::Relaxed) {
            let bytes = self.receive(offer, URI_LIST, MAX_URI_LIST_BYTES)?;
            return files::read_uri_list(&bytes);
        }
        Ok(None)
    }

    fn receive(
        &self,
        offer: &wl_data_offer::WlDataOffer,
        mime: &str,
        limit: u64,
    ) -> io::Result<Vec<u8>> {
        let (reader, writer) = UnixStream::pair()?;
        reader.set_read_timeout(Some(Duration::from_secs(4)))?;
        offer.receive(mime.to_owned(), writer.as_fd());
        self.queue.flush().map_err(io::Error::other)?;
        drop(writer);

        let mut bytes = Vec::new();
        reader.take(limit + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(io::Error::other(tr("clipboard-image-too-large")));
        }
        Ok(bytes)
    }
}

impl State {
    fn bind_seat(
        &mut self,
        registry: &wl_registry::WlRegistry,
        global: &Global,
        handle: &QueueHandle<Self>,
    ) {
        if global.interface == wl_seat::WlSeat::interface().name {
            let seat = registry.bind(global.name, global.version.min(5), handle, ());
            let device = self.manager.get_data_device(&seat, handle, global.name);
            self.seats.insert(
                global.name,
                Seat {
                    seat,
                    device,
                    selection: None,
                },
            );
        }
    }
}

impl Drop for Seat {
    fn drop(&mut self) {
        self.set_selection(None);
        if self.device.version() >= 2 {
            self.device.release();
        }
        if self.seat.version() >= 5 {
            self.seat.release();
        }
    }
}

impl Seat {
    fn set_selection(&mut self, offer: Option<wl_data_offer::WlDataOffer>) {
        if let Some(previous) = std::mem::replace(&mut self.selection, offer) {
            previous.destroy();
        }
    }
}

delegate_noop!(State: ignore wl_seat::WlSeat);
delegate_noop!(State: ignore wl_data_device_manager::WlDataDeviceManager);

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                state.bind_seat(
                    registry,
                    &Global {
                        name,
                        interface,
                        version,
                    },
                    handle,
                );
            }
            wl_registry::Event::GlobalRemove { name } => {
                state.seats.remove(&name);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_data_device::WlDataDevice, u32> for State {
    event_created_child!(State, wl_data_device::WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (wl_data_offer::WlDataOffer, ImageFormats::default()),
    ]);

    fn event(
        state: &mut Self,
        _: &wl_data_device::WlDataDevice,
        event: wl_data_device::Event,
        seat: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_device::Event::Selection { id } => {
                if let Some(seat) = state.seats.get_mut(seat) {
                    seat.set_selection(id);
                } else if let Some(offer) = id {
                    offer.destroy();
                }
            }
            // winit handles drag-and-drop on its own data device.
            wl_data_device::Event::Enter {
                id: Some(offer), ..
            } => offer.destroy(),
            _ => {}
        }
    }
}

impl Dispatch<wl_data_offer::WlDataOffer, ImageFormats> for State {
    fn event(
        _: &mut Self,
        _: &wl_data_offer::WlDataOffer,
        event: wl_data_offer::Event,
        formats: &ImageFormats,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_offer::Event::Offer { mime_type } = event {
            match mime_type.as_str() {
                PNG => formats.png.store(true, Ordering::Relaxed),
                URI_LIST => formats.uri_list.store(true, Ordering::Relaxed),
                _ => {}
            }
        }
    }
}
