use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsStr,
    fs,
    os::unix::{
        ffi::OsStrExt,
        fs::{FileTypeExt, MetadataExt},
    },
    path::{Path, PathBuf},
};

use rustix::fs::{AtFlags, Mode, OFlags, StatxFlags, fstat, fstatvfs, major, minor, open, statx};

use crate::model::DiskUsage;

pub(super) fn sample() -> Vec<DiskUsage> {
    let sys = Path::new("/sys");
    let mut disks = read_disks(sys, Path::new("/dev/disk/by-id"));
    if !disks.is_empty()
        && let Ok(mounts) = fs::read("/proc/self/mountinfo")
    {
        add_usage(sys, &mut disks, &mounts, filesystem_usage);
    }

    // Multiple paths to the same hardware can report the same WWID.
    let mut by_id = BTreeMap::<_, DiskUsage>::new();
    for disk in disks.into_values() {
        by_id
            .entry(disk.id.clone())
            .and_modify(|first| first.used = first.used.max(disk.used))
            .or_insert(disk);
    }
    let mut drives: Vec<_> = by_id.into_values().collect();
    drives.sort_by(|a, b| a.device.cmp(&b.device));
    drives
}

fn read_disks(sys: &Path, by_id: &Path) -> BTreeMap<PathBuf, DiskUsage> {
    let mut aliases = HashMap::new();
    for entry in entries(by_id) {
        if let Ok(target) = fs::read_link(entry.path())
            && let Some(device) = target.file_name()
        {
            let name = entry.file_name();
            let alias = aliases
                .entry(device.to_owned())
                .or_insert_with(|| name.clone());
            if name < *alias {
                *alias = name;
            }
        }
    }
    let devices = sys.join("devices");
    entries(&sys.join("block"))
        .filter_map(|entry| {
            let path = entry.path().canonicalize().ok()?;
            let location = path.strip_prefix(&devices).ok()?;
            let name = entry.file_name();
            let name = name.to_str()?;
            if location.starts_with("virtual")
                || name
                    .strip_prefix("mmcblk")
                    .is_some_and(|suffix| !suffix.bytes().all(|byte| byte.is_ascii_digit()))
            {
                return None;
            }
            let sectors = read_value(&path.join("size"))?.parse::<u64>().ok()?;
            let total = sectors.checked_mul(512).filter(|total| *total > 0)?;
            let id = read_value(&path.join("wwid"))
                .or_else(|| read_value(&path.join("device/wwid")))
                .map(|wwid| format!("wwid:{wwid}"))
                .or_else(|| {
                    aliases
                        .get(OsStr::new(name))
                        .map(|alias| format!("by-id:{}", alias.to_string_lossy()))
                })
                .unwrap_or_else(|| format!("device:{name}"));
            let device = format!("/dev/{name}");
            let label = read_value(&path.join("device/model"))
                .or_else(|| read_value(&path.join("device/name")))
                .unwrap_or_else(|| device.clone());
            Some((
                path,
                DiskUsage {
                    id,
                    label,
                    device,
                    used: None,
                    total,
                },
            ))
        })
        .collect()
}

struct Mount<'a> {
    id: u64,
    major: u32,
    minor: u32,
    target: PathBuf,
    kind: &'a [u8],
    source: PathBuf,
}

fn parse_mount(line: &[u8]) -> Option<Mount<'_>> {
    let mut fields = line.split(|byte| *byte == b' ');
    let id = std::str::from_utf8(fields.next()?).ok()?.parse().ok()?;
    let numbers = std::str::from_utf8(fields.nth(1)?).ok()?;
    let (major, minor) = numbers.split_once(':')?;
    let target = mount_path(fields.nth(1)?);
    fields.find(|field| *field == b"-")?;
    Some(Mount {
        id,
        major: major.parse().ok()?,
        minor: minor.parse().ok()?,
        target,
        kind: fields.next()?,
        source: mount_path(fields.next()?),
    })
}

fn mount_path(field: &[u8]) -> PathBuf {
    let mut bytes = Vec::with_capacity(field.len());
    let mut remaining = field;
    while let Some((&first, rest)) = remaining.split_first() {
        match remaining {
            [
                b'\\',
                a @ b'0'..=b'3',
                b @ b'0'..=b'7',
                c @ b'0'..=b'7',
                tail @ ..,
            ] => {
                bytes.push((a - b'0') * 64 + (b - b'0') * 8 + (c - b'0'));
                remaining = tail;
            }
            _ => {
                bytes.push(first);
                remaining = rest;
            }
        }
    }
    PathBuf::from(OsStr::from_bytes(&bytes))
}

fn add_usage(
    sys: &Path,
    disks: &mut BTreeMap<PathBuf, DiskUsage>,
    mountinfo: &[u8],
    read_usage: impl Fn(&Mount<'_>) -> Option<u64>,
) {
    let mut seen = HashSet::new();
    for mount in mountinfo
        .split(|byte| *byte == b'\n')
        .filter_map(parse_mount)
    {
        let Some((filesystem, disk)) = mounted_disk(sys, disks, &mount) else {
            continue;
        };
        if !seen.contains(&filesystem)
            && let Some(used) = read_usage(&mount)
        {
            seen.insert(filesystem);
            if let Some(disk) = disks.get_mut(&disk) {
                disk.used = Some(disk.used.unwrap_or_default().saturating_add(used));
            }
        }
    }
}

fn mounted_disk(
    sys: &Path,
    disks: &BTreeMap<PathBuf, DiskUsage>,
    mount: &Mount<'_>,
) -> Option<(PathBuf, PathBuf)> {
    let device = sys
        .join(format!("dev/block/{}:{}", mount.major, mount.minor))
        .canonicalize()
        .ok()
        .or_else(|| {
            // These filesystems may use an anonymous device number in mountinfo.
            matches!(mount.kind, b"btrfs" | b"fuseblk")
                .then(|| source_device(sys, &mount.source))
                .flatten()
        })?;
    if mount.kind == b"btrfs" {
        // Subvolumes share usage. Count the filesystem only if every member is on one drive.
        for entry in entries(&sys.join("fs/btrfs")) {
            let Ok(members) = member_paths(&entry.path().join("devices")) else {
                continue;
            };
            if members.contains(&device) {
                return Some((entry.path(), common_disk(&members, disks)?.to_path_buf()));
            }
        }
        return None;
    }
    let disk = backing_disk(&device, disks)?.to_path_buf();
    Some((device, disk))
}

fn source_device(sys: &Path, source: &Path) -> Option<PathBuf> {
    let metadata = fs::metadata(source).ok()?;
    if !metadata.file_type().is_block_device() {
        return None;
    }
    let dev = metadata.rdev();
    sys.join(format!("dev/block/{}:{}", major(dev), minor(dev)))
        .canonicalize()
        .ok()
}

fn backing_disk<'a>(device: &Path, disks: &'a BTreeMap<PathBuf, DiskUsage>) -> Option<&'a Path> {
    if let Some((path, _)) = disks.get_key_value(device) {
        return Some(path);
    }
    if device.join("partition").exists() {
        return backing_disk(device.parent()?, disks);
    }
    // dm-crypt, LVM, and RAID expose their backing devices here.
    let members = member_paths(&device.join("slaves")).ok()?;
    common_disk(&members, disks)
}

fn common_disk<'a>(
    devices: &[PathBuf],
    disks: &'a BTreeMap<PathBuf, DiskUsage>,
) -> Option<&'a Path> {
    let (first, remaining) = devices.split_first()?;
    let parent = backing_disk(first, disks)?;
    remaining.iter().try_for_each(|device| {
        let other = backing_disk(device, disks)?;
        (disks.get(parent)?.id == disks.get(other)?.id).then_some(())
    })?;
    Some(parent)
}

fn filesystem_usage(mount: &Mount<'_>) -> Option<u64> {
    let fd = open(&mount.target, OFlags::PATH | OFlags::CLOEXEC, Mode::empty()).ok()?;
    // Check for overmounts by mount ID. Btrfs reports a different st_dev for each subvolume.
    let visible = match statx(&fd, "", AtFlags::EMPTY_PATH, StatxFlags::MNT_ID) {
        Ok(info) if StatxFlags::from_bits_retain(info.stx_mask).contains(StatxFlags::MNT_ID) => {
            info.stx_mnt_id == mount.id
        }
        _ => {
            // Older kernels may not return a mount ID.
            let dev = fstat(&fd).ok()?.st_dev;
            (major(dev), minor(dev)) == (mount.major, mount.minor)
        }
    };
    if !visible {
        return None;
    }
    let stats = fstatvfs(&fd).ok()?;
    Some(
        stats
            .f_blocks
            .saturating_sub(stats.f_bfree)
            .saturating_mul(stats.f_frsize),
    )
}

fn member_paths(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    fs::read_dir(path)?
        .map(|entry| entry?.path().canonicalize())
        .collect()
}

fn entries(path: &Path) -> impl Iterator<Item = fs::DirEntry> {
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
}

fn read_value(path: &Path) -> Option<String> {
    let value = fs::read_to_string(path).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use std::{
        os::unix::fs::symlink,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn checks_mount_visibility_even_when_the_inode_device_differs() {
        let mountinfo = fs::read("/proc/self/mountinfo").unwrap();
        let mut root = mountinfo
            .split(|byte| *byte == b'\n')
            .filter_map(parse_mount)
            .find(|mount| mount.target == Path::new("/") && filesystem_usage(mount).is_some())
            .expect("visible root mount");
        let minor = root.minor;
        // Model a Btrfs subvolume whose inode and superblock use different device numbers.
        root.minor ^= 1;
        assert!(filesystem_usage(&root).is_some());

        root.minor = minor;
        root.id += 1;
        assert_eq!(filesystem_usage(&root), None);
    }

    #[test]
    fn counts_filesystems_once_on_their_drive_and_keeps_unmounted_drives() {
        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture = Fixture(std::env::temp_dir().join(format!("secondscreen-storage-{nonce}")));
        let sys = &fixture.0;
        fs::create_dir_all(sys.join("block")).unwrap();
        fs::create_dir_all(sys.join("dev/block")).unwrap();
        fs::create_dir_all(sys.join("by-id")).unwrap();
        let disk = |name: &str, location: &str| {
            let path = sys.join(location).join(name);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("size"), "2048\n").unwrap();
            symlink(&path, sys.join("block").join(name)).unwrap();
            path
        };
        let sda = disk("sda", "devices/pci/block");
        let sdb = disk("sdb", "devices/pci/block");
        let loop0 = disk("loop0", "devices/virtual/block");
        fs::write(sda.join("wwid"), "example-wwid").unwrap();
        symlink("../../sdb", sys.join("by-id/usb-example")).unwrap();

        let sda1 = sda.join("sda1");
        let sda2 = sda.join("sda2");
        let sda3 = sda.join("sda3");
        for (part, number) in [(&sda1, "1"), (&sda2, "2"), (&sda3, "3")] {
            fs::create_dir(part).unwrap();
            fs::write(part.join("partition"), number).unwrap();
        }
        let encrypted = sys.join("devices/virtual/block/dm-0");
        fs::create_dir_all(encrypted.join("slaves")).unwrap();
        symlink(&sda3, encrypted.join("slaves/sda3")).unwrap();
        let btrfs_devices = sys.join("fs/btrfs/root-filesystem/devices");
        fs::create_dir_all(&btrfs_devices).unwrap();
        symlink(&encrypted, btrfs_devices.join("dm-0")).unwrap();
        let shared = sys.join("devices/virtual/block/dm-1");
        fs::create_dir_all(shared.join("slaves")).unwrap();
        symlink(&sda3, shared.join("slaves/sda3")).unwrap();
        symlink(&sdb, shared.join("slaves/sdb")).unwrap();

        for (number, device) in [
            ("8:1", &sda1),
            ("8:2", &sda2),
            ("253:0", &encrypted),
            ("253:1", &shared),
            ("7:0", &loop0),
        ] {
            symlink(device, sys.join("dev/block").join(number)).unwrap();
        }

        let mut drives = read_disks(sys, &sys.join("by-id"));
        assert_eq!(drives.len(), 2);
        add_usage(
            sys,
            &mut drives,
            br"10 1 253:0 /root /sysroot ro - btrfs /dev/dm-0 rw
11 1 253:0 /root/ostree/deploy/fedora/deploy/example / ro - btrfs /dev/dm-0 rw
12 11 253:0 /var /var rw - btrfs /dev/dm-0 rw
13 11 8:2 / /boot rw - ext4 /dev/sda2 rw
14 13 8:1 / /boot/efi rw - vfat /dev/sda1 rw
15 11 8:2 /subdir /boot\040duplicate rw - ext4 /dev/sda2 rw
16 11 253:1 / /shared rw - ext4 /dev/dm-1 rw
17 11 7:0 / /image ro - squashfs /dev/loop0 ro
18 11 0:5 / /network rw - tmpfs tmpfs rw",
            |mount| match mount.target.to_str().unwrap() {
                "/sysroot" => None,
                "/" => Some(500_000),
                "/boot" => Some(200),
                "/boot/efi" => Some(100),
                unexpected => panic!("unexpected usage lookup for {unexpected}"),
            },
        );
        assert_eq!(drives[&sda].id, "wwid:example-wwid");
        assert_eq!(drives[&sda].total, 1_048_576);
        assert_eq!(drives[&sda].used, Some(500_300));
        assert_eq!(drives[&sdb].id, "by-id:usb-example");
        assert_eq!(drives[&sdb].used, None);
    }
}
