const timer = new QTimer();
timer.interval = 75;
// Give the caller time to report a failed move, then clean up if it has exited.
const deadline = Date.now() + 12000;

function finish() {
    timer.stop();
    callDBus('org.kde.KWin', '/Scripting', 'org.kde.kwin.Scripting', 'unloadScript', pluginName);
}

function placeWindow() {
    if (Date.now() >= deadline) {
        finish();
        return;
    }

    const output = workspace.screens.find(screen => screen.name === outputName);
    const window = workspace.windowList().find(window => window.pid === pid && window.resourceClass === appId);
    if (!output || !window)
        return;

    if (!window.output || window.output.name !== outputName)
        workspace.sendClientToScreen(window, output);
    if (window.fullScreen !== fullscreen)
        window.fullScreen = fullscreen;
    if (window.output && window.output.name === outputName && window.fullScreen === fullscreen)
        finish();
}

timer.timeout.connect(placeWindow);
timer.start();
