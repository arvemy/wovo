use leptos::prelude::*;
use leptos_ui::clx;

#[derive(Clone, Copy, PartialEq, Default)]
pub enum CardSize {
    #[default]
    Default,
    Sm,
}

mod components {
    use super::*;
    clx! {CardHeader, div, "flex flex-col gap-1.5 px-6"}
    clx! {CardTitle, h2, "leading-none font-semibold"}
    clx! {CardDescription, p, "text-muted-foreground text-sm"}
    clx! {CardContent, div, "px-6"}
    clx! {CardFooter, footer, "flex items-center gap-2 px-6"}
}

#[allow(unused_imports)]
pub use components::*;

#[component]
pub fn Card(
    #[prop(optional)] size: CardSize,
    #[prop(into, optional)] class: String,
    children: Children,
) -> impl IntoView {
    let (size_class, px_class) = match size {
        CardSize::Default => ("py-6 gap-4", "px-6"),
        CardSize::Sm => ("py-4 gap-3", "px-4"),
    };
    let data_size = match size {
        CardSize::Default => "default",
        CardSize::Sm => "sm",
    };
    let merged = tw_merge::tw_merge!(
        "bg-card text-card-foreground flex flex-col rounded-xl border shadow-sm",
        size_class,
        px_class,
        class
    );

    view! {
        <div class=merged data-size=data_size data-name="Card">
            {children()}
        </div>
    }
}
