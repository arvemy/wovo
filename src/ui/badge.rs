use leptos::prelude::*;
use leptos_ui::variants;

variants! {
    Badge {
        base: "inline-flex items-center font-semibold rounded-md border transition-colors focus:outline-hidden focus:ring-2 focus:ring-ring focus:ring-offset-2 w-fit",
        variants: {
            variant: {
                Default: "border-transparent shadow bg-primary text-primary-foreground hover:bg-primary/80",
                Secondary: "border-transparent bg-secondary text-secondary-foreground hover:bg-secondary/80",
                Accent: "border-transparent bg-accent text-accent-foreground hover:bg-accent/80",
                Muted: "border-transparent bg-muted text-muted-foreground hover:bg-muted/80",
                Destructive: "border-transparent shadow bg-destructive text-destructive-foreground hover:bg-destructive/80",
                Outline: "text-foreground",
                Success: "border-transparent bg-[var(--success-muted)] text-[var(--success-foreground)]",
                Warning: "border-transparent bg-[var(--warning-muted)] text-[var(--warning-foreground)]",
                Info: "border-transparent bg-muted text-muted-foreground",
            },
            size: {
                Default: "px-2.5 py-0.5 text-xs",
                Sm: "px-1.5 py-0.5 text-[10px]",
                Lg: "px-3 py-1 text-sm",
            }
        },
        component: {
            element: span
        }
    }
}
