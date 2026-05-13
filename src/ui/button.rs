use leptos::prelude::*;
use leptos_ui::variants;

variants! {
    Button {
        base: "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 shrink-0 [&_svg]:shrink-0 outline-none focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] w-fit hover:cursor-pointer active:scale-[0.98] active:opacity-100 select-none",
        variants: {
            variant: {
                Default: "bg-primary text-primary-foreground shadow-xs hover:bg-primary/90",
                Destructive: "bg-destructive text-destructive-foreground shadow-xs hover:bg-destructive/90",
                Outline: "border bg-background shadow-xs hover:bg-accent hover:text-accent-foreground",
                Secondary: "bg-secondary text-secondary-foreground shadow-xs hover:bg-secondary/80",
                Ghost: "hover:bg-accent hover:text-accent-foreground",
                Link: "text-primary underline-offset-4 hover:underline",
            },
            size: {
                Default: "h-9 px-4 py-2 has-[>svg]:px-3",
                Sm: "h-8 rounded-md gap-1.5 px-3 has-[>svg]:px-2.5",
                Lg: "h-10 rounded-md px-6 has-[>svg]:px-4",
                Icon: "size-9",
            }
        },
        component: {
            element: button
        }
    }
}
