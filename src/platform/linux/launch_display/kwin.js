const timer = new QTimer();
timer.interval = 75;
const deadline = Date.now() + 5000;

function finish() {
    timer.stop();
    callDBus('org.kde.KWin', '/Scripting', 'org.kde.kwin.Scripting', 'unloadScript', pluginName);
}

function placeWindow() {
    const output = workspace.screens.find(screen => screen.name === outputName);
    if (!output) {
        finish();
        return;
    }

    const window = workspace.windowList().find(window => window.pid === pid && window.resourceClass === appId);
    if (window) {
        if (window.output.name !== outputName)
            workspace.sendClientToScreen(window, output);
        finish();
    } else if (Date.now() >= deadline) {
        console.warn('Second Screen did not find its window for display placement.');
        finish();
    }
}

timer.timeout.connect(placeWindow);
timer.start();
