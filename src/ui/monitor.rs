use crate::model::DiskUsage;

use super::percent;

mod chart;
mod meter;
pub(super) use chart::monitor_graph;
pub(super) use chart::{discard_charts, graph};
pub(super) use meter::{gauge, monitoring_bar};

pub(super) fn drive_load(disk: &DiskUsage) -> Option<f32> {
    disk.used
        .filter(|&used| used <= disk.total)
        .and_then(|used| percent(used, disk.total))
}

pub(super) fn gib(bytes: u64) -> f64 {
    bytes as f64 / (1024. * 1024. * 1024.)
}

pub(super) fn capacity(bytes: u64) -> String {
    let gib = gib(bytes);
    if gib >= 1024. {
        format!("{:.1} TiB", gib / 1024.)
    } else if gib >= 1. {
        format!("{gib:.0} GiB")
    } else {
        format!("{:.0} MiB", bytes as f64 / (1024. * 1024.))
    }
}

pub(super) fn rate(bytes: f32) -> (String, &'static str) {
    if bytes >= 1024. * 1024. * 1024. {
        (format!("{:.1}", bytes / (1024. * 1024. * 1024.)), "GiB/s")
    } else if bytes >= 1024. * 1024. {
        (format!("{:.1}", bytes / (1024. * 1024.)), "MiB/s")
    } else if bytes >= 1024. {
        (format!("{:.1}", bytes / 1024.), "KiB/s")
    } else {
        (format!("{bytes:.0}"), "B/s")
    }
}

pub(super) fn uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = seconds / 3600 % 24;
    let minutes = seconds / 60 % 60;
    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else {
        format!("{hours}h {minutes}m")
    }
}
