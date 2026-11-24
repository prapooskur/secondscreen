use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    process, thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use rustix::fs::{FlockOperation, flock};
use zbus::blocking::{Proxy, connection::Builder};

use crate::platform::APP_ID;

use super::{RETRY_INTERVAL, WINDOW_TIMEOUT};

const KWIN: &str = "org.kde.KWin";

pub(super) fn place_window(display: &str, fullscreen: bool) -> Result<()> {
    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("No Wayland runtime directory is available")?
        .join(APP_ID);
    fs::create_dir_all(&runtime)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(runtime.join("kwin-scripting.lock"))?;
    // KWin assigns script IDs from its active script count. Keep our loads and unloads in order.
    flock(&lock, FlockOperation::LockExclusive)?;

    let pid = process::id();
    let name = format!("{APP_ID}.{pid}");
    let mut file = tempfile::NamedTempFile::new_in(runtime)?;
    writeln!(
        file,
        "const pid = {pid};\nconst appId = {app};\nconst outputName = {output};\nconst fullscreen = {fullscreen};\nconst pluginName = {plugin};\n{script}",
        app = serde_json::to_string(APP_ID)?,
        output = serde_json::to_string(display)?,
        plugin = serde_json::to_string(&name)?,
        script = include_str!("kwin.js"),
    )?;
    let path = file.path().to_str().context("Invalid KWin script path")?;
    let connection = Builder::session()?
        .method_timeout(Duration::from_secs(2))
        .build()?;
    let scripting = Proxy::new(&connection, KWIN, "/Scripting", "org.kde.kwin.Scripting")?;
    let script_id: i32 = scripting.call("loadScript", &(path, &name))?;
    ensure!(
        script_id >= 0,
        "KWin did not load the display placement script"
    );
    let result = (|| -> Result<()> {
        let script = Proxy::new(
            &connection,
            KWIN,
            format!("/Scripting/Script{script_id}"),
            "org.kde.kwin.Script",
        )?;
        script.call::<_, _, ()>("run", &())?;
        let deadline = Instant::now() + WINDOW_TIMEOUT + RETRY_INTERVAL * 2;
        while scripting.call::<_, _, bool>("isScriptLoaded", &name)? {
            ensure!(
                Instant::now() < deadline,
                "KWin did not finish display placement"
            );
            thread::sleep(RETRY_INTERVAL);
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = scripting.call::<_, _, bool>("unloadScript", &name);
    }
    result
}
