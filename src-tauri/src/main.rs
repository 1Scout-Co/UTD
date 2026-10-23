#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if app_lib::native_host::invoked() {
        std::process::exit(app_lib::native_host::run());
    }
    app_lib::run();
}
