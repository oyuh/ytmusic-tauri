// Stream Deck plugin for YT Music (ytmusic-tauri).
// Each button press maps to a GET on the app's local control server. That's the
// whole plugin — no state, no polling, so it stays fast and idle-free.
const CONTROL_BASE = 'http://127.0.0.1:7897';

const ROUTES = {
  'com.lawson.ytmusic.playpause': '/playpause',
  'com.lawson.ytmusic.prev': '/prev',
  'com.lawson.ytmusic.next': '/next',
  'com.lawson.ytmusic.volup': '/volup',
  'com.lawson.ytmusic.voldown': '/voldown',
};

// Called by the Stream Deck host when the plugin loads.
function connectElgatoStreamDeckSocket(inPort, inPluginUUID, inRegisterEvent, _inInfo) {
  const ws = new WebSocket('ws://127.0.0.1:' + inPort);

  ws.onopen = () => {
    ws.send(JSON.stringify({ event: inRegisterEvent, uuid: inPluginUUID }));
  };

  ws.onmessage = (evt) => {
    let msg;
    try { msg = JSON.parse(evt.data); } catch (e) { return; }
    if (msg.event !== 'keyDown') return;
    const path = ROUTES[msg.action];
    if (!path) return;
    // If the app isn't running the fetch just fails — nothing to do.
    fetch(CONTROL_BASE + path).catch(() => {});
  };
}
