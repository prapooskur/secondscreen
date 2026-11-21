use std::{fs, path::Path};

use crate::model::Sensor;

pub(super) fn hardware(root: &Path, sensors: &mut Vec<Sensor>) {
    let Ok(devices) = fs::read_dir(root) else {
        return;
    };
    let mut hardware = Vec::new();
    for device in devices.flatten() {
        let path = device.path();
        let Ok(name) = fs::read_to_string(path.join("name")) else {
            continue;
        };
        let name = name.trim();
        let Ok(real) = path.canonicalize() else {
            continue;
        };
        // hwmonN is allocated at boot. The physical device path and channel persist.
        let parent = real.parent().and_then(Path::parent).unwrap_or(&real);
        let Ok(files) = fs::read_dir(&path) else {
            continue;
        };
        for entry in files.flatten() {
            let file = entry.file_name();
            let Some(channel) = file.to_str().and_then(|s| s.strip_suffix("_input")) else {
                continue;
            };
            let prefix = channel.trim_end_matches(|c: char| c.is_ascii_digit());
            let (unit, divisor) = match prefix {
                "temp" => ("°C", 1000.),
                "fan" => ("RPM", 1.),
                "in" => ("V", 1000.),
                "curr" => ("A", 1000.),
                "power" => ("W", 1_000_000.),
                "freq" => ("MHz", 1_000_000.),
                "humidity" => ("%", 1000.),
                _ => continue,
            };
            let label = fs::read_to_string(path.join(format!("{channel}_label")))
                .unwrap_or_else(|_| channel.to_owned());
            let value = fs::read_to_string(entry.path())
                .ok()
                .and_then(|s| s.trim().parse::<f32>().ok())
                .map(|n| n / divisor);
            hardware.push(Sensor::new(
                format!("hwmon:{}:{name}:{channel}", parent.display()),
                format!("{name} · {}", label.trim()),
                unit,
                value,
            ));
        }
    }
    hardware.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    sensors.extend(hardware);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hardware_labels_units_and_stable_device_ids() {
        let dir = std::env::temp_dir().join(format!("secondscreen-hwmon-{}", std::process::id()));
        let chip = dir.join("devices/pci/hwmon/hwmon4");
        let class = dir.join("class");
        fs::create_dir_all(&chip).unwrap();
        fs::create_dir_all(&class).unwrap();
        std::os::unix::fs::symlink(&chip, class.join("hwmon4")).unwrap();
        for (key, value) in [
            ("name", "board"),
            ("in1_input", "1200"),
            ("in1_label", "Vcore"),
            ("fan2_input", "906"),
            ("temp1_input", "-3500"),
        ] {
            fs::write(chip.join(key), value).unwrap();
        }
        let mut data = Vec::new();
        hardware(&class, &mut data);
        assert_eq!(data.len(), 3);
        let voltage = data.iter().find(|s| s.unit == "V").unwrap();
        assert_eq!(voltage.name, "board · Vcore");
        assert_eq!(voltage.value, Some(1.2));
        assert!(!voltage.id.contains("hwmon4"));
        assert_eq!(
            data.iter().find(|s| s.unit == "°C").unwrap().value,
            Some(-3.5)
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
