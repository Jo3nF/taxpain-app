// Evita una consola extra en Windows en release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    taxpain_desktop_lib::run()
}
