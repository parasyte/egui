use crate::*;

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "persistence", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "persistence", serde(default))]
pub(crate) struct State {
    /// Positive offset means scrolling down/right
    offset: Vec2,

    show_scroll: bool,

    /// Momentum, used for kinetic scrolling
    #[cfg_attr(feature = "persistence", serde(skip))]
    pub vel: Vec2,
    /// Mouse offset relative to the top of the handle when started moving the handle.
    scroll_start_offset_from_top: Option<f32>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            offset: Vec2::ZERO,
            show_scroll: false,
            vel: Vec2::ZERO,
            scroll_start_offset_from_top: None,
        }
    }
}

// TODO: rename VScroll
/// Add vertical scrolling to a contained [`Ui`].
#[derive(Clone, Debug)]
#[must_use = "You should call .show()"]
pub struct ScrollArea {
    max_height: f32,
    always_show_scroll: bool,
    id_source: Option<Id>,
    offset: Option<Vec2>,
}

impl ScrollArea {
    /// Will make the area be as high as it is allowed to be (i.e. fill the [`Ui`] it is in)
    pub fn auto_sized() -> Self {
        Self::from_max_height(f32::INFINITY)
    }

    /// Use `f32::INFINITY` if you want the scroll area to expand to fit the surrounding Ui
    pub fn from_max_height(max_height: f32) -> Self {
        Self {
            max_height,
            always_show_scroll: false,
            id_source: None,
            offset: None,
        }
    }

    /// If `false` (default), the scroll bar will be hidden when not needed/
    /// If `true`, the scroll bar will always be displayed even if not needed.
    pub fn always_show_scroll(mut self, always_show_scroll: bool) -> Self {
        self.always_show_scroll = always_show_scroll;
        self
    }

    /// A source for the unique `Id`, e.g. `.id_source("second_scroll_area")` or `.id_source(loop_index)`.
    pub fn id_source(mut self, id_source: impl std::hash::Hash) -> Self {
        self.id_source = Some(Id::new(id_source));
        self
    }

    /// Set the vertical scroll offset position.
    ///
    /// See also: [`Ui::scroll_to_cursor`](crate::ui::Ui::scroll_to_cursor) and
    /// [`Response::scroll_to_me`](crate::Response::scroll_to_me)
    pub fn scroll_offset(mut self, offset: f32) -> Self {
        self.offset = Some(Vec2::new(0.0, offset));
        self
    }
}

struct Prepared {
    id: Id,
    state: State,
    always_show_scroll: bool,
    rect: Rect,
    content_ui: Ui,
    /// Relative coordinates: the offset and size of the view of the inner UI.
    /// `viewport.min == ZERO` means we scrolled to the top.
    viewport: Rect,
}

impl ScrollArea {
    fn begin(self, ui: &mut Ui) -> Prepared {
        let Self {
            max_height,
            always_show_scroll,
            id_source,
            offset,
        } = self;

        let ctx = ui.ctx().clone();

        let id_source = id_source.unwrap_or_else(|| Id::new("scroll_area"));
        let id = ui.make_persistent_id(id_source);
        let mut state = *ctx.memory().id_data.get_or_default::<State>(id);

        if let Some(offset) = offset {
            state.offset = offset;
        }

        // content: size of contents (generally large; that's why we want scroll bars)
        // rect: size of scroll area. The area we clip the contents to.

        let available_outer = ui.available_rect_before_wrap();

        let outer_size = vec2(
            available_outer.width(),
            available_outer.height().at_most(max_height),
        );

        let rect = Rect::from_min_size(available_outer.min, outer_size);

        let mut content_ui = ui.child_ui(
            Rect::from_min_size(rect.min - state.offset, vec2(outer_size.x, f32::INFINITY)),
            *ui.layout(),
        );
        let mut content_clip_rect = rect.expand(ui.visuals().clip_rect_margin);
        content_clip_rect = content_clip_rect.intersect(ui.clip_rect());
        content_clip_rect.max.x = ui.clip_rect().max.x; // Nice handling of forced resizing beyond the possible
        content_ui.set_clip_rect(content_clip_rect);

        let viewport = Rect::from_min_size(Pos2::ZERO + state.offset, outer_size);

        Prepared {
            id,
            state,
            always_show_scroll,
            rect,
            content_ui,
            viewport,
        }
    }

    /// Show the `ScrollArea`, and add the contents to the viewport.
    ///
    /// If the inner area can be very long, consider using [`Self::show_rows`] instead.
    pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
        self.show_viewport(ui, |ui, _viewport| add_contents(ui))
    }

    /// Efficiently show only the visible part of a large number of rows.
    ///
    /// ```
    /// # let ui = &mut egui::Ui::__test();
    /// let text_style = egui::TextStyle::Body;
    /// let row_height = ui.fonts()[text_style].row_height();
    /// // let row_height = ui.spacing().interact_size.y; // if you are adding buttons instead of labels.
    /// let num_rows = 10_000;
    /// egui::ScrollArea::auto_sized().show_rows(ui, row_height, num_rows, |ui, row_range| {
    ///     for row in row_range {
    ///         let text = format!("Row {}/{}", row + 1, num_rows);
    ///         ui.label(text);
    ///     }
    /// });
    pub fn show_rows<R>(
        self,
        ui: &mut Ui,
        row_height_sans_spacing: f32,
        num_rows: usize,
        add_contents: impl FnOnce(&mut Ui, std::ops::Range<usize>) -> R,
    ) -> R {
        let spacing = ui.spacing().item_spacing;
        let row_height_with_spacing = row_height_sans_spacing + spacing.y;
        self.show_viewport(ui, |ui, viewport| {
            ui.set_height((row_height_with_spacing * num_rows as f32 - spacing.y).at_least(0.0));

            let min_row = (viewport.min.y / row_height_with_spacing)
                .floor()
                .at_least(0.0) as usize;
            let max_row = (viewport.max.y / row_height_with_spacing).ceil() as usize + 1;
            let max_row = max_row.at_most(num_rows);

            let y_min = ui.max_rect().top() + min_row as f32 * row_height_with_spacing;
            let y_max = ui.max_rect().top() + max_row as f32 * row_height_with_spacing;
            let mut viewport_ui = ui.child_ui(
                Rect::from_x_y_ranges(ui.max_rect().x_range(), y_min..=y_max),
                *ui.layout(),
            );

            viewport_ui.skip_ahead_auto_ids(min_row); // Make sure we get consistent IDs.

            add_contents(&mut viewport_ui, min_row..max_row)
        })
    }

    /// This can be used to only paint the visible part of the contents.
    ///
    /// `add_contents` is past the viewport, which is the relative view of the content.
    /// So if the passed rect has min = zero, then show the top left content (the user has not scrolled).
    pub fn show_viewport<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui, Rect) -> R) -> R {
        let mut prepared = self.begin(ui);
        let ret = add_contents(&mut prepared.content_ui, prepared.viewport);
        prepared.end(ui);
        ret
    }
}

impl Prepared {
    fn end(self, ui: &mut Ui) {
        let Prepared {
            id,
            mut state,
            rect,
            always_show_scroll,
            content_ui,
            viewport: _,
        } = self;

        let content_size = content_ui.min_size();

        // We take the scroll target so only this ScrollArea will use it.
        let scroll_target = content_ui.ctx().frame_state().scroll_target.take();
        if let Some((scroll_y, align)) = scroll_target {
            let center_factor = align.to_factor();

            let top = content_ui.min_rect().top();
            let visible_range = top..=top + content_ui.clip_rect().height();
            let offset_y = scroll_y - lerp(visible_range, center_factor);

            let mut spacing = ui.spacing().item_spacing.y;

            // Depending on the alignment we need to add or subtract the spacing
            spacing *= remap(center_factor, 0.0..=1.0, -1.0..=1.0);

            state.offset.y = offset_y + spacing;
        }

        let width = if rect.width().is_finite() {
            rect.width().max(content_size.x) // Expand width to fit content
        } else {
            // ScrollArea is in an infinitely wide parent
            content_size.x
        };

        let rect = Rect::from_min_size(rect.min, vec2(width, rect.height()));

        let content_is_too_small = content_size.y > rect.height();

        let max_offset = content_size.y - rect.height();
        if ui.rect_contains_pointer(rect) {
            let mut frame_state = ui.ctx().frame_state();
            let scroll_delta = frame_state.scroll_delta;

            let scrolling_up = state.offset.y > 0.0 && scroll_delta.y > 0.0;
            let scrolling_down = state.offset.y < max_offset && scroll_delta.y < 0.0;

            if scrolling_up || scrolling_down {
                state.offset.y -= scroll_delta.y;
                // Clear scroll delta so no parent scroll will use it.
                frame_state.scroll_delta = Vec2::ZERO;
            }
        }

        let show_scroll_this_frame = content_is_too_small || always_show_scroll;
        let max_scroll_bar_width = ui.spacing().scroll_bar_width;

        let dragging = if show_scroll_this_frame {
            let right = rect.right() + max_scroll_bar_width * 0.25;
            let left = right - max_scroll_bar_width;
            let top = rect.top();
            let bottom = rect.bottom();

            let mut scroll_rect = Rect::from_min_max(pos2(left, top), pos2(right, bottom));

            let from_content =
                |content_y| remap_clamp(content_y, 0.0..=content_size.y, top..=bottom);

            let handle_rect = Rect::from_min_max(
                pos2(left, from_content(state.offset.y)),
                pos2(right, from_content(state.offset.y + rect.height())),
            );

            let interact_id = id.with("vertical");
            let response = ui.interact(scroll_rect, interact_id, Sense::click_and_drag());

            if let Some(pointer_pos) = response.interact_pointer_pos() {
                let scroll_start_offset_from_top =
                    state.scroll_start_offset_from_top.get_or_insert_with(|| {
                        if handle_rect.contains(pointer_pos) {
                            pointer_pos.y - handle_rect.top()
                        } else {
                            let handle_top_pos_at_bottom = bottom - handle_rect.height();
                            // Calculate the new handle top position, centering the handle on the mouse.
                            let new_handle_top_pos = (pointer_pos.y - handle_rect.height() / 2.0)
                                .clamp(top, handle_top_pos_at_bottom);
                            pointer_pos.y - new_handle_top_pos
                        }
                    });

                let new_handle_top = pointer_pos.y - *scroll_start_offset_from_top;
                state.offset.y = remap(new_handle_top, top..=bottom, 0.0..=content_size.y);
            } else {
                state.scroll_start_offset_from_top = None;
            }

            let unbounded_offset_y = state.offset.y;
            state.offset.y = state.offset.y.max(0.0);
            state.offset.y = state.offset.y.min(max_offset);

            if state.offset.y != unbounded_offset_y {
                state.vel = Vec2::ZERO;
            }

            // Avoid frame-delay by calculating a new handle rect:
            let mut handle_rect = Rect::from_min_max(
                pos2(left, from_content(state.offset.y)),
                pos2(right, from_content(state.offset.y + rect.height())),
            );
            let min_handle_height = ui.spacing().scroll_bar_width;
            if handle_rect.size().y < min_handle_height {
                handle_rect = Rect::from_center_size(
                    handle_rect.center(),
                    vec2(handle_rect.size().x, min_handle_height),
                );
            }

            let hovered_width = max_scroll_bar_width
                * 0.75
                * ui.ctx()
                    .animate_bool(id.with("hovered"), !(response.hovered || response.dragged));
            scroll_rect.min.x += hovered_width;
            handle_rect.min.x += hovered_width;

            let visuals = ui.style().interact(&response);

            ui.painter().add(epaint::Shape::rect_filled(
                scroll_rect,
                visuals.corner_radius,
                ui.visuals().extreme_bg_color,
            ));

            ui.painter().add(epaint::Shape::rect_filled(
                handle_rect,
                visuals.corner_radius,
                visuals.bg_fill,
            ));

            response.dragged
        } else {
            false
        };

        if content_is_too_small && !dragging {
            // Drag contents to scroll (for touch screens mostly):
            let content_response = ui.interact(rect, id.with("area"), Sense::drag());

            let input = ui.input();
            if content_response.dragged() {
                state.offset.y -= input.pointer.delta().y;
                state.vel = input.pointer.velocity();
            } else {
                let stop_speed = 20.0; // Pixels per second.
                let friction_coeff = 1000.0; // Pixels per second squared.
                let dt = input.unstable_dt;

                let friction = friction_coeff * dt;
                if friction > state.vel.length() || state.vel.length() < stop_speed {
                    state.vel = Vec2::ZERO;
                } else {
                    state.vel -= friction * state.vel.normalized();
                    // Offset has an inverted coordinate system compared to
                    // the velocity, so we subtract it instead of adding it
                    state.offset.y -= state.vel.y * dt;
                    ui.ctx().request_repaint();
                }
            }
        }

        let size = vec2(
            rect.size().x,
            rect.size().y.min(content_size.y), // shrink if content is so small that we don't need scroll bars
        );
        ui.advance_cursor_after_rect(Rect::from_min_size(rect.min, size));

        if show_scroll_this_frame != state.show_scroll {
            ui.ctx().request_repaint();
        }

        state.offset.y = state.offset.y.min(content_size.y - rect.height());
        state.offset.y = state.offset.y.max(0.0);
        state.show_scroll = show_scroll_this_frame;

        ui.memory().id_data.insert(id, state);
    }
}
