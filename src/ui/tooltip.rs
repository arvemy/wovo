use leptos::prelude::*;
use leptos_ui::clx;
use tw_merge::tw_merge;

clx! {Tooltip, div, "inline-block relative mx-0 whitespace-nowrap transition-all duration-300 ease-in-out group/tooltip"}

#[derive(Clone, Copy, Default, strum::Display, strum::AsRefStr)]
#[allow(dead_code)]
pub enum TooltipPosition {
    #[default]
    Top,
    Left,
    Right,
    Bottom,
}

#[component]
pub fn TooltipContent(
    #[prop(into, optional)] class: String,
    #[prop(default = TooltipPosition::default())] position: TooltipPosition,
    children: Children,
) -> impl IntoView {
    const SHARED: &str = "absolute opacity-0 transition-all duration-300 ease-in-out pointer-events-none group-hover/tooltip:opacity-100 group-hover/tooltip:pointer-events-auto z-[1000000]";

    let position_class = match position {
        TooltipPosition::Top => "left-1/2 bottom-full mb-1 -ml-2.5",
        TooltipPosition::Right => "bottom-1/2 left-full ml-2.5 -mb-3.5",
        TooltipPosition::Bottom => "left-1/2 top-full mt-1 -ml-2.5",
        TooltipPosition::Left => "bottom-1/2 right-full mr-2.5 -mb-3.5",
    };

    let arrow_position_class = match position {
        TooltipPosition::Top => "left-1/2 bottom-full -mb-2 border-t-foreground/90",
        TooltipPosition::Right => "bottom-1/2 left-full -mr-0.5 -mb-1 border-r-foreground/90",
        TooltipPosition::Bottom => "left-1/2 top-full -mt-2 border-b-foreground/90",
        TooltipPosition::Left => "bottom-1/2 right-full -mb-1 -ml-0.5 border-l-foreground/90",
    };

    let tooltip_class = tw_merge!(
        SHARED,
        "py-2 px-2.5 text-xs whitespace-nowrap shadow-lg text-background bg-foreground/90",
        class,
        position_class,
    );

    let arrow_class = tw_merge!(
        SHARED,
        "bg-transparent border-transparent border-6",
        arrow_position_class,
    );

    view! {
        <>
            <div data-name="TooltipArrow" data-position=position.to_string() class=arrow_class />
            <div data-name="TooltipContent" data-position=position.to_string() class=tooltip_class>
                {children()}
            </div>
        </>
    }
}
