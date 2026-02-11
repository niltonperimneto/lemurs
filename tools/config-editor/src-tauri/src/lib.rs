// Use loose typing for max flexibility in the editor
#[tauri::command]
fn get_config() -> Result<serde_json::Value, String> {
    // Path relative to where `cargo tauri dev` runs (tools/config-editor)
    // Or relative to the binary
    let path = std::path::Path::new("../../extra/config.toml");

    // Fallback attempts
    let content = if path.exists() {
        std::fs::read_to_string(path).map_err(|e| e.to_string())?
    } else {
        // Try absolute path for dev env if relative fails (though running from the correct dir usually works)
        return Err(format!(
            "Config file not found at {:?}",
            path.canonicalize()
        ));
    };

    let toml_val: toml::Value = toml::from_str(&content).map_err(|e| e.to_string())?;

    // Convert TOML AST to JSON AST for the frontend
    let json_val = serde_json::to_value(toml_val).map_err(|e| e.to_string())?;

    Ok(json_val)
}

#[tauri::command]
fn save_config(config: serde_json::Value) -> Result<(), String> {
    let path = std::path::Path::new("../../extra/config.toml");

    // Convert JSON AST back to TOML AST (approximation)
    // Note: `serde_json::Value` -> `toml::Value` conversion via serde
    // Direct conversion works but some types might be ambiguous (float vs int).
    // `toml` crate handles this reasonably well.
    let toml_val: toml::Value = serde_json::from_value(config).map_err(|e| e.to_string())?;

    let toml_str = toml::to_string_pretty(&toml_val).map_err(|e| e.to_string())?;

    std::fs::write(path, toml_str).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_config, save_config])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
