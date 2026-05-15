use leptos::prelude::*;
use tw_merge::tw_merge;

#[derive(Clone, Copy, Default, PartialEq)]
#[expect(dead_code, reason = "component API supports vertical separators")]
pub enum SeparatorOrientation {
    #[default]
    Horizontal,
    Vertical,
}

#[component]
pub fn Separator(
    #[prop(optional)] orientation: SeparatorOrientation,
    #[prop(into, optional)] class: String,
) -> impl IntoView {
    let (aria_orientation, orientation_class) = match orientation {
        SeparatorOrientation::Horizontal => ("horizontal", "w-full h-[1px]"),
        SeparatorOrientation::Vertical => ("vertical", "h-full w-[1px]"),
    };
    let merged = tw_merge!("shrink-0 bg-border", orientation_class, class);
    view! {
        <div data-name="Separator" role="separator" aria-orientation=aria_orientation class=merged />
    }
}
