// Без консольного окна в релизе.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    zapret_gui_lib::run()
}
