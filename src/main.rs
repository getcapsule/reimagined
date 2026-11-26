#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod player_ui;

fn main() -> anyhow::Result<()> {
    let player = capsule::audio::Player::new()?;
    let paths = std::env::args_os().skip(1).map(std::path::PathBuf::from);
    player_ui::run(player, paths)?;
    Ok(())
}
