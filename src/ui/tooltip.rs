use leptos::html::Div;
use leptos::portal::Portal;
use leptos::prelude::*;
use send_wrapper::SendWrapper;
use tw_merge::tw_merge;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{Element, Event, HtmlDivElement, ResizeObserver};

const TOOLTIP_GAP: f64 = 8.0;
const VIEWPORT_MARGIN: f64 = 8.0;
const ARROW_INSET: f64 = 10.0;

#[derive(Clone, Copy)]
struct TooltipState {
    trigger_ref: NodeRef<Div>,
    is_open: ReadSignal<bool>,
}

#[derive(Clone, Debug, PartialEq)]
struct TooltipLayout {
    style: String,
    arrow_style: String,
    ready: bool,
}

struct TooltipSubscriptions {
    window: web_sys::Window,
    layout_handler: Closure<dyn FnMut(Event)>,
    _resize_observer_handler: Closure<dyn FnMut(js_sys::Array, ResizeObserver)>,
    resize_observer: Option<ResizeObserver>,
}

impl Default for TooltipLayout {
    fn default() -> Self {
        Self {
            style: "top: 0px; left: 0px;".to_string(),
            arrow_style: "left: 50%; top: -4px;".to_string(),
            ready: false,
        }
    }
}

#[component]
pub fn Tooltip(#[prop(into, optional)] class: String, children: Children) -> impl IntoView {
    let trigger_ref = NodeRef::<Div>::new();
    let (is_open, set_is_open) = signal(false);

    provide_context(TooltipState {
        trigger_ref,
        is_open,
    });

    let merged_classes = tw_merge!("inline-block relative mx-0 whitespace-nowrap", class,);

    view! {
        <div
            class=merged_classes
            data-name="Tooltip"
            node_ref=trigger_ref
            on:pointerenter=move |_| set_is_open.set(true)
            on:pointerleave=move |_| set_is_open.set(false)
            on:focusin=move |_| set_is_open.set(true)
            on:focusout=move |_| set_is_open.set(false)
        >
            {children()}
        </div>
    }
}

#[derive(Clone, Copy, Default, strum::Display, strum::AsRefStr)]
#[allow(dead_code)]
pub enum TooltipPosition {
    #[default]
    Top,
    Left,
    Right,
    Bottom,
}

#[derive(Clone, Copy, Default, strum::Display, strum::AsRefStr)]
#[allow(dead_code)]
pub enum TooltipAlign {
    Start,
    #[default]
    Center,
    End,
}

#[component]
pub fn TooltipContent(
    #[prop(into, optional)] class: String,
    #[prop(default = TooltipPosition::default())] position: TooltipPosition,
    #[prop(default = TooltipAlign::default())] align: TooltipAlign,
    children: ChildrenFn,
) -> impl IntoView {
    let content_ref = NodeRef::<Div>::new();
    let (layout, set_layout) = signal(TooltipLayout::default());
    let state = use_context::<TooltipState>();

    if let Some(state) = state {
        Effect::new(move |_| {
            if !state.is_open.get() {
                return;
            }

            update_tooltip_layout(state.trigger_ref, content_ref, position, align, set_layout);

            let Some(window) = web_sys::window() else {
                return;
            };

            let layout_handler = Closure::<dyn FnMut(Event)>::new(move |_| {
                update_tooltip_layout(state.trigger_ref, content_ref, position, align, set_layout);
            });

            let _ = window.add_event_listener_with_callback(
                "resize",
                layout_handler.as_ref().unchecked_ref(),
            );
            let _ = window.add_event_listener_with_callback_and_bool(
                "scroll",
                layout_handler.as_ref().unchecked_ref(),
                true,
            );

            let resize_observer_handler =
                Closure::<dyn FnMut(js_sys::Array, ResizeObserver)>::new(move |_, _| {
                    update_tooltip_layout(
                        state.trigger_ref,
                        content_ref,
                        position,
                        align,
                        set_layout,
                    );
                });
            let resize_observer =
                ResizeObserver::new(resize_observer_handler.as_ref().unchecked_ref()).ok();

            if let Some(observer) = resize_observer.as_ref() {
                if let Some(trigger) = state.trigger_ref.get() {
                    observer.observe(trigger.unchecked_ref::<Element>());
                }
                if let Some(content) = content_ref.get() {
                    observer.observe(content.unchecked_ref::<Element>());
                }
            }

            let subscriptions = SendWrapper::new(TooltipSubscriptions {
                window,
                layout_handler,
                _resize_observer_handler: resize_observer_handler,
                resize_observer,
            });

            on_cleanup(move || {
                subscriptions.take().cleanup();
            });
        });
    }

    let wrapper_class = move || {
        let is_ready = layout.get().ready;
        let is_visible = state.map(|state| state.is_open.get()).unwrap_or(false) && is_ready;

        tw_merge!(
            "fixed left-0 top-0 z-[1000000] w-max max-w-[calc(100vw-1rem)] pointer-events-none transition-opacity duration-150 ease-in-out",
            if is_visible { "opacity-100" } else { "opacity-0" },
            if is_ready { "" } else { "invisible" },
        )
    };

    let tooltip_class = tw_merge!(
        "py-2 px-2.5 text-xs whitespace-nowrap shadow-lg text-background bg-foreground/90",
        class,
    );

    let arrow_class = "absolute size-2 bg-foreground/90";

    view! {
        <Portal>
            <div
                data-name="TooltipLayer"
                data-position=position.to_string()
                data-align=align.to_string()
                class=wrapper_class
                style=move || layout.get().style
                node_ref=content_ref
            >
                <div data-name="TooltipArrow" class=arrow_class style=move || layout.get().arrow_style />
                <div data-name="TooltipContent" class=tooltip_class.clone()>
                    {children()}
                </div>
            </div>
        </Portal>
    }
}

impl TooltipSubscriptions {
    fn cleanup(self) {
        let _ = self.window.remove_event_listener_with_callback(
            "resize",
            self.layout_handler.as_ref().unchecked_ref(),
        );
        let _ = self.window.remove_event_listener_with_callback_and_bool(
            "scroll",
            self.layout_handler.as_ref().unchecked_ref(),
            true,
        );

        if let Some(observer) = self.resize_observer {
            observer.disconnect();
        }
    }
}

fn calculate_layout(
    trigger: &HtmlDivElement,
    content: &HtmlDivElement,
    position: TooltipPosition,
    align: TooltipAlign,
) -> TooltipLayout {
    let trigger_rect = trigger
        .unchecked_ref::<Element>()
        .get_bounding_client_rect();
    let content_rect = content
        .unchecked_ref::<Element>()
        .get_bounding_client_rect();
    let content_width = content_rect.width();
    let content_height = content_rect.height();
    let viewport_width = window()
        .inner_width()
        .ok()
        .and_then(|value| value.as_f64())
        .unwrap_or(content_width + VIEWPORT_MARGIN * 2.0);
    let viewport_height = window()
        .inner_height()
        .ok()
        .and_then(|value| value.as_f64())
        .unwrap_or(content_height + VIEWPORT_MARGIN * 2.0);

    let trigger_center_x = trigger_rect.left() + trigger_rect.width() / 2.0;
    let trigger_center_y = trigger_rect.top() + trigger_rect.height() / 2.0;

    let raw_left = match position {
        TooltipPosition::Top | TooltipPosition::Bottom => match align {
            TooltipAlign::Start => trigger_rect.left(),
            TooltipAlign::Center => trigger_center_x - content_width / 2.0,
            TooltipAlign::End => trigger_rect.right() - content_width,
        },
        TooltipPosition::Left => trigger_rect.left() - content_width - TOOLTIP_GAP,
        TooltipPosition::Right => trigger_rect.right() + TOOLTIP_GAP,
    };

    let raw_top = match position {
        TooltipPosition::Top => trigger_rect.top() - content_height - TOOLTIP_GAP,
        TooltipPosition::Bottom => trigger_rect.bottom() + TOOLTIP_GAP,
        TooltipPosition::Left | TooltipPosition::Right => trigger_center_y - content_height / 2.0,
    };

    let left = clamp_to_viewport(raw_left, content_width, viewport_width);
    let top = clamp_to_viewport(raw_top, content_height, viewport_height);

    TooltipLayout {
        style: format!("top: {top:.1}px; left: {left:.1}px;"),
        arrow_style: arrow_style(
            position,
            trigger_center_x - left,
            trigger_center_y - top,
            content_width,
            content_height,
        ),
        ready: true,
    }
}

fn update_tooltip_layout(
    trigger_ref: NodeRef<Div>,
    content_ref: NodeRef<Div>,
    position: TooltipPosition,
    align: TooltipAlign,
    set_layout: WriteSignal<TooltipLayout>,
) {
    let Some(trigger) = trigger_ref.get() else {
        return;
    };
    let Some(content) = content_ref.get() else {
        return;
    };

    set_layout.set(calculate_layout(&trigger, &content, position, align));
}

fn clamp_to_viewport(value: f64, size: f64, viewport_size: f64) -> f64 {
    let max = (viewport_size - size - VIEWPORT_MARGIN).max(VIEWPORT_MARGIN);
    value.clamp(VIEWPORT_MARGIN, max)
}

fn clamp_arrow_offset(value: f64, size: f64) -> f64 {
    value.clamp(ARROW_INSET, (size - ARROW_INSET).max(ARROW_INSET))
}

fn arrow_style(
    position: TooltipPosition,
    trigger_offset_x: f64,
    trigger_offset_y: f64,
    content_width: f64,
    content_height: f64,
) -> String {
    match position {
        TooltipPosition::Top => {
            let left = clamp_arrow_offset(trigger_offset_x, content_width);
            format!("left: {left:.1}px; bottom: -4px; transform: translateX(-50%) rotate(45deg);")
        }
        TooltipPosition::Bottom => {
            let left = clamp_arrow_offset(trigger_offset_x, content_width);
            format!("left: {left:.1}px; top: -4px; transform: translateX(-50%) rotate(45deg);")
        }
        TooltipPosition::Left => {
            let top = clamp_arrow_offset(trigger_offset_y, content_height);
            format!("top: {top:.1}px; right: -4px; transform: translateY(-50%) rotate(45deg);")
        }
        TooltipPosition::Right => {
            let top = clamp_arrow_offset(trigger_offset_y, content_height);
            format!("top: {top:.1}px; left: -4px; transform: translateY(-50%) rotate(45deg);")
        }
    }
}
