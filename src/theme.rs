use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = ["window", "__WOVO_THEME"], js_name = get)]
    fn get_theme_preference() -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_namespace = ["window", "__WOVO_THEME"], js_name = set)]
    fn set_theme_preference_raw(mode: &str) -> Result<JsValue, JsValue>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThemeMode {
    Light,
    Dark,
    Auto,
}

impl ThemeMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Light => "Light Mode",
            Self::Dark => "Dark Mode",
            Self::Auto => "Auto (System)",
        }
    }

    pub(crate) fn storage_value(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::Auto => "auto",
        }
    }

    fn from_storage_value(value: &str) -> Option<Self> {
        match value {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

pub(crate) fn current_theme_preference() -> ThemeMode {
    get_theme_preference()
        .ok()
        .and_then(|value| value.as_string())
        .and_then(|value| ThemeMode::from_storage_value(&value))
        .unwrap_or(ThemeMode::Auto)
}

pub(crate) fn set_theme_preference(mode: &str) -> Result<JsValue, JsValue> {
    set_theme_preference_raw(mode)
}

pub(crate) fn theme_menu_item_class(selected: bool) -> &'static str {
    if selected {
        "flex h-8 w-full items-center justify-between rounded-sm bg-accent px-2 text-sm font-medium text-accent-foreground transition-colors"
    } else {
        "flex h-8 w-full items-center justify-between rounded-sm px-2 text-sm font-medium text-muted-foreground transition-colors hover:cursor-pointer hover:bg-accent hover:text-accent-foreground"
    }
}
