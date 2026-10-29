// Each area is an inlined plugin. Its commands are read from the `generate_handler![...]` in its mod.rs, and its
// default permission allows them all, so a row adding a command only edits its own area.
const AREAS: [&str; 7] = ["input", "games", "library", "system", "settings", "startup", "media"];

fn main() {
    let mut attrs = tauri_build::Attributes::new();
    for area in AREAS {
        let path = format!("src/{area}/mod.rs");
        println!("cargo:rerun-if-changed={path}");
        let src = std::fs::read_to_string(&path).unwrap_or_default();
        let plugin = tauri_build::InlinedPlugin::new()
            .commands(Box::leak(commands(&src).into_boxed_slice()))
            .default_permission(tauri_build::DefaultPermissionRule::AllowAllCommands);
        attrs = attrs.plugin(area, plugin);
    }
    tauri_build::try_build(attrs).expect("tauri-build failed");
}

fn commands(src: &str) -> Vec<&'static str> {
    let Some(start) = src.find("generate_handler![") else { return Vec::new() };
    let list = &src[start + "generate_handler![".len()..];
    let list = &list[..list.find(']').unwrap_or(0)];
    list.split(',')
        .filter_map(|c| c.trim().rsplit("::").next())
        .filter(|c| !c.is_empty())
        .map(|c| &*Box::leak(c.to_string().into_boxed_str()))
        .collect()
}
