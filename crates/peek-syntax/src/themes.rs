#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorRgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl ColorRgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn to_hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

#[derive(Debug, Clone)]
pub struct EditorTheme {
    pub name: &'static str,
    pub background: ColorRgb,
    pub sidebar_background: ColorRgb,
    pub foreground: ColorRgb,
    pub gutter_foreground: ColorRgb,
    pub gutter_active: ColorRgb,
    pub selection_background: ColorRgb,
    pub cursor_color: ColorRgb,
    pub border_color: ColorRgb,
}

impl EditorTheme {
    /// Modern Fluent Dark Minimalist Theme (Windows 11 inspired)
    pub const fn fluent_dark() -> Self {
        Self {
            name: "Fluent Dark",
            background: ColorRgb::new(18, 18, 20), // #121214 deep charcoal
            sidebar_background: ColorRgb::new(24, 24, 27), // #18181b
            foreground: ColorRgb::new(212, 212, 216), // #d4d4d8
            gutter_foreground: ColorRgb::new(82, 82, 91), // #52525b
            gutter_active: ColorRgb::new(228, 228, 231), // #e4e4e7
            selection_background: ColorRgb::new(39, 39, 42), // #27272a
            cursor_color: ColorRgb::new(59, 130, 246), // #3b82f6
            border_color: ColorRgb::new(39, 39, 42), // #27272a
        }
    }
}
