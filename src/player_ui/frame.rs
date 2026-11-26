use super::Message;
use iced::{
    Color, Element, Length, Task,
    widget::{button, container, row, space::horizontal as horizontal_space, text},
    window,
};
use raw_window_handle::RawWindowHandle;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::{Dwm::*, Gdi::ScreenToClient},
    UI::{
        HiDpi::GetDpiForWindow,
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

const HEIGHT: u32 = 32;
const CONTROL: u32 = 44;

#[derive(Debug, Clone, Copy)]
pub(super) enum Action {
    Minimize,
    Maximize,
    Close,
}

pub(super) fn controls(maximized: bool) -> Element<'static, Message> {
    let control = |glyph, action| {
        super::material::button(
            container(
                text(glyph)
                    .size(11)
                    .font(iced::Font::with_name("Segoe MDL2 Assets")),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
        .padding(0)
        .width(CONTROL as f32)
        .height(HEIGHT as f32)
        .on_press(Message::Window(action))
        .style(move |_, state| button::Style {
            background: match state {
                button::Status::Hovered | button::Status::Pressed => Some(
                    if matches!(action, Action::Close) {
                        Color::from_rgb8(178, 49, 74)
                    } else {
                        Color::from_rgb8(57, 54, 73)
                    }
                    .into(),
                ),
                _ => None,
            },
            text_color: Color::from_rgb8(222, 218, 232),
            border: iced::Border::default().rounded(super::material::RADIUS),
            ..Default::default()
        })
    };
    row![
        horizontal_space(),
        control("\u{e921}", Action::Minimize),
        control(
            if maximized { "\u{e923}" } else { "\u{e922}" },
            Action::Maximize
        ),
        control("\u{e8bb}", Action::Close)
    ]
    .height(HEIGHT as f32)
    .into()
}

pub(super) fn perform(action: Action) -> Task<Message> {
    window::oldest().and_then(move |id| match action {
        Action::Minimize => window::minimize(id, true),
        Action::Maximize => window::toggle_maximize(id),
        Action::Close => window::close(id),
    })
}

pub(super) fn install() -> Task<Message> {
    window::oldest().and_then(|id| {
        window::run(id, |window| {
            let Ok(handle) = window.window_handle() else {
                return Message::FrameUnavailable;
            };
            let RawWindowHandle::Win32(raw) = handle.as_raw() else {
                return Message::FrameUnavailable;
            };
            let hwnd = raw.hwnd.get() as HWND;
            // The window callback owns a live HWND on its event thread.
            unsafe {
                let border = DWMWA_COLOR_NONE;
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_BORDER_COLOR as u32,
                    (&raw const border).cast(),
                    size_of_val(&border) as u32,
                );
                let dark = 1i32;
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                    (&raw const dark).cast(),
                    size_of_val(&dark) as u32,
                );
                let backdrop = DWMSBT_NONE;
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_SYSTEMBACKDROP_TYPE as u32,
                    (&raw const backdrop).cast(),
                    size_of_val(&backdrop) as u32,
                );
                if SetWindowSubclass(hwnd, Some(hit_test), 1, 0) == 0 {
                    return Message::FrameUnavailable;
                }
            }
            Message::Tick
        })
    })
}

unsafe extern "system" fn hit_test(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    // The system supplies the window handle and message parameters on the owning thread.
    unsafe {
        if msg == WM_NCHITTEST {
            let mut rect = RECT::default();
            let mut cursor = POINT {
                x: l as i16 as i32,
                y: (l >> 16) as i16 as i32,
            };
            if GetClientRect(hwnd, &mut rect) != 0 && ScreenToClient(hwnd, &mut cursor) != 0 {
                let scale = GetDpiForWindow(hwnd).max(96) as i32;
                let px = |logical: u32| (logical as i32 * scale + 48) / 96;
                let x = cursor.x;
                let y = cursor.y;
                let width = rect.right - rect.left;
                let height = rect.bottom - rect.top;
                if IsZoomed(hwnd) == 0 {
                    let edge = px(5);
                    let hit = match (x < edge, x >= width - edge, y < edge, y >= height - edge) {
                        (true, _, true, _) => HTTOPLEFT,
                        (_, true, true, _) => HTTOPRIGHT,
                        (true, _, _, true) => HTBOTTOMLEFT,
                        (_, true, _, true) => HTBOTTOMRIGHT,
                        (true, _, _, _) => HTLEFT,
                        (_, true, _, _) => HTRIGHT,
                        (_, _, true, _) => HTTOP,
                        (_, _, _, true) => HTBOTTOM,
                        _ => HTCLIENT,
                    };
                    if hit != HTCLIENT {
                        return hit as LRESULT;
                    }
                }
                if y < px(HEIGHT)
                    && x >= px(super::SIDEBAR_CONTROL_WIDTH)
                    && x < width - px(3 * CONTROL)
                {
                    return HTCAPTION as LRESULT;
                }
                return HTCLIENT as LRESULT;
            }
        } else if msg == WM_NCDESTROY {
            RemoveWindowSubclass(hwnd, Some(hit_test), id);
        }
        DefSubclassProc(hwnd, msg, w, l)
    }
}
