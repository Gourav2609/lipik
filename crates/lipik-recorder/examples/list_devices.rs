//! List input devices and the microphone permission status.

use lipik_recorder::{Permission, list_input_devices, permission_status};

fn main() -> Result<(), lipik_recorder::Error> {
    let permission = permission_status();
    if permission == Permission::Denied {
        eprintln!("Microphone access denied. Enable it in System Settings.");
    }
    println!("permission: {permission:?}");

    for d in list_input_devices()? {
        let mark = if d.is_default { "*" } else { " " };
        let channels = d.channels.map_or("?".into(), |c| c.to_string());
        println!(
            "{mark} {} [{}] {channels} ch  id={}",
            d.name, d.backend, d.id
        );
    }
    Ok(())
}
