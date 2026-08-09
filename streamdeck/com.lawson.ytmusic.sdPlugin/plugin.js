// Stream Deck plugin for YT Music (ytmusic-tauri).
// - keyDown on each action -> GET the app's local control server.
// - polls /status so the play/pause button reflects state (play vs pause icon)
//   and the volume buttons show the current volume %.
const CONTROL_BASE = 'http://127.0.0.1:7897';
const PLAYPAUSE = 'com.lawson.ytmusic.playpause';
const VOLUP = 'com.lawson.ytmusic.volup';
const VOLDOWN = 'com.lawson.ytmusic.voldown';

const ROUTES = {
  [PLAYPAUSE]: '/playpause',
  'com.lawson.ytmusic.prev': '/prev',
  'com.lawson.ytmusic.next': '/next',
  [VOLUP]: '/volup',
  [VOLDOWN]: '/voldown',
};

let ws = null;
const contexts = {}; // action UUID -> Set of visible button contexts
let lastStatus = null;

function send(obj) {
  if (ws && ws.readyState === 1) ws.send(JSON.stringify(obj));
}

function apply(action, ctx, s) {
  if (!s) return;
  if (action === PLAYPAUSE) {
    send({ event: 'setState', context: ctx, payload: { state: s.playing ? 1 : 0 } });
  } else if (action === VOLUP || action === VOLDOWN) {
    send({ event: 'setTitle', context: ctx, payload: { title: (s.volume | 0) + '%', target: 0 } });
  }
}

async function poll() {
  let s;
  try {
    s = await (await fetch(CONTROL_BASE + '/status')).json();
  } catch (e) {
    return; // app not running - leave buttons as-is
  }
  lastStatus = s;
  for (const action in contexts) for (const ctx of contexts[action]) apply(action, ctx, s);
}

function connectElgatoStreamDeckSocket(inPort, inPluginUUID, inRegisterEvent, _inInfo) {
  ws = new WebSocket('ws://127.0.0.1:' + inPort);

  ws.onopen = () => ws.send(JSON.stringify({ event: inRegisterEvent, uuid: inPluginUUID }));

  ws.onmessage = (evt) => {
    let m;
    try { m = JSON.parse(evt.data); } catch (e) { return; }
    switch (m.event) {
      case 'keyDown': {
        const path = ROUTES[m.action];
        if (path) fetch(CONTROL_BASE + path).catch(() => {});
        break;
      }
      case 'willAppear':
        (contexts[m.action] = contexts[m.action] || new Set()).add(m.context);
        apply(m.action, m.context, lastStatus);
        break;
      case 'willDisappear':
        if (contexts[m.action]) contexts[m.action].delete(m.context);
        break;
    }
  };

  setInterval(poll, 1500);
}
