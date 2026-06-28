// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
  // Move the process cwd to the user's home directory so the AI doesn't
  // start operating in the project's own source tree by default.
  if let Some(home) = dirs::home_dir() {
    let _ = std::env::set_current_dir(&home);
  }
  meyatu_code_lib::run();
}
