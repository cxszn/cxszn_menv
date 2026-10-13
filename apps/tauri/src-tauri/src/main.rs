fn main() {
    if let Some(exit_code) = cxszn_menv_tauri_lib::run_php_shim_if_invoked() {
        std::process::exit(exit_code);
    }
    if let Some(exit_code) = cxszn_menv_tauri_lib::run_native_cli_if_requested() {
        std::process::exit(exit_code);
    }
    cxszn_menv_tauri_lib::run();
}
