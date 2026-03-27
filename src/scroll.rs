//! Scrollable wrapper widget for vertical content.

use xkbcommon::xkb::Keysym;

use hayate_ui::platform::keyboard::KeyState;
use hayate_ui::scroll::delegate::ItemRect;
use hayate_ui::widget::core::{Constraints, EventResponse, Size, Widget, WidgetEvent};

pub(crate) struct ScrollableWidget {
    pub(crate) inner: Box<dyn Widget>,
    pub(crate) scroll_offset: f32,
    pub(crate) viewport_height: f32,
    content_height: f32,
    line_height: f32,
}

impl ScrollableWidget {
    pub(crate) fn new(inner: Box<dyn Widget>, line_height: f32) -> Self {
        Self {
            inner,
            scroll_offset: 0.0,
            viewport_height: 0.0,
            content_height: 0.0,
            line_height,
        }
    }

    pub(crate) fn scroll_by(&mut self, delta: f32) {
        let max_offset = (self.content_height - self.viewport_height).max(0.0);
        self.scroll_offset = (self.scroll_offset + delta).clamp(0.0, max_offset);
    }

    pub(crate) fn reset_scroll(&mut self) {
        self.scroll_offset = 0.0;
    }
}

impl Widget for ScrollableWidget {
    fn layout(&mut self, constraints: &Constraints) -> Size {
        let inner_constraints = Constraints {
            min_width: constraints.min_width,
            max_width: constraints.max_width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };
        let inner_size = self.inner.layout(&inner_constraints);
        self.content_height = inner_size.height;
        self.viewport_height = constraints.max_height;
        Size::new(inner_size.width, constraints.max_height)
    }

    fn paint(&self, canvas: &mut [u8], rect: ItemRect, stride: u32) {
        let shifted_rect = ItemRect::new(
            rect.x,
            rect.y - self.scroll_offset,
            rect.width,
            self.content_height,
        );
        self.inner.paint(canvas, shifted_rect, stride);
    }

    fn event(&mut self, event: &WidgetEvent) -> EventResponse {
        match event {
            WidgetEvent::Scroll { dy, .. } => {
                self.scroll_by(*dy);
                EventResponse::Handled
            }
            WidgetEvent::Key(ke) if ke.state == KeyState::Pressed => {
                let delta = match ke.keysym {
                    Keysym::Up => -self.line_height,
                    Keysym::Down => self.line_height,
                    Keysym::Page_Up => -self.viewport_height * 0.8,
                    Keysym::Page_Down => self.viewport_height * 0.8,
                    Keysym::Home => -self.scroll_offset,
                    Keysym::End => self.content_height,
                    _ => return self.inner.event(event),
                };
                self.scroll_by(delta);
                EventResponse::Handled
            }
            // Adjust pointer Y for scroll offset before passing to inner
            WidgetEvent::PointerPress { x, y, button, modifiers, .. } => {
                let adjusted = WidgetEvent::PointerPress {
                    x: *x,
                    y: *y + self.scroll_offset,
                    button: *button,
                    modifiers: *modifiers,
                };
                self.inner.event(&adjusted)
            }
            _ => self.inner.event(event),
        }
    }

    fn dirty(&self) -> bool {
        true
    }
}
