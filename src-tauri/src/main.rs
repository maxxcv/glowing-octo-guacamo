#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod download;

use tauri::{generate_handler, Builder};

fn main() {
  Builder::default()
    .invoke_handler(generate_handler![
      download::download,
      download::cancel_download
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
