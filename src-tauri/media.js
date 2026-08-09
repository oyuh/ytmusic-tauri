// Media bridge: reports what YT Music is playing to the Rust side, which mirrors it
// to Windows SMTC (the media popup) under the "YT Music" identity. WebView2's own
// SMTC session is disabled (see additional_browser_args) so there's no duplicate.
//
// Reads from YT Music's #movie_player (always present) rather than only
// navigator.mediaSession, so it's robust. Emits only when the track or play/pause
// state changes, not every tick.
(function () {
  if (window.top !== window.self) return;

  // Perceptual volume curve. YT Music's slider is linear, so low numbers are still
  // loud. We deliberately DON'T override the player's volume API: YT's native volume
  // (0-100) stays the single source of truth, so the slider, getVolume, and the Stream
  // Deck all read the exact same number and stay in sync. We only enforce the actual
  // output amplitude to (volume/100)^2. It's re-applied on whatever <video> element is
  // current (so it survives song switches, which is where it used to jump loud) and
  // whenever the volume changes.
  const VOL_EXP = 2;
  function enforceVolumeCurve() {
    const p = document.querySelector('#movie_player');
    const v = document.querySelector('video');
    if (!p || !v || typeof p.getVolume !== 'function') return;
    const curved = () => Math.pow(Math.max(0, Math.min(100, p.getVolume())) / 100, VOL_EXP);
    const t = curved();
    if (Math.abs(v.volume - t) > 0.0006) v.volume = t;
    if (!v.__ytmCurve) {
      v.__ytmCurve = true;
      v.addEventListener('volumechange', () => {
        const t2 = curved();
        if (Math.abs(v.volume - t2) > 0.0006) v.volume = t2;
      });
    }
  }

  let last = '';
  let lastEmit = 0;
  function poll() {
    const p = document.querySelector('#movie_player');
    if (!p || !p.getPlayerState) return;
    let d = {};
    try { d = p.getVideoData() || {}; } catch (e) {}
    const title = d.title || '';
    if (!title) return;
    const artist = d.author || '';
    const playing = p.getPlayerState() === 1; // 1 = playing

    let art = '';
    const md = navigator.mediaSession && navigator.mediaSession.metadata;
    if (md && md.artwork && md.artwork.length) art = md.artwork[md.artwork.length - 1].src;
    else if (d.video_id) art = 'https://i.ytimg.com/vi/' + d.video_id + '/hqdefault.jpg';

    // Album (for Last.fm art matching). YT Music sets mediaSession.album for songs;
    // fall back to the player-bar byline "Artist • Album • Year".
    let album = (md && md.album) || '';
    if (!album) {
      const bl = document.querySelector('ytmusic-player-bar .byline, ytmusic-player-bar yt-formatted-string.byline');
      if (bl) {
        const parts = bl.textContent.split('•').map(s => s.trim()).filter(Boolean);
        if (parts.length >= 3 && /^\d{4}$/.test(parts[parts.length - 1])) album = parts[parts.length - 2];
      }
    }

    let pos = 0, dur = 0, volume = 0;
    try { pos = p.getCurrentTime() || 0; dur = p.getDuration() || 0; } catch (e) {}
    try { volume = Math.round(p.getVolume ? p.getVolume() : 0); } catch (e) {}

    // Emit on any change to track / play-state / volume, and at least every ~10s
    // while playing so Rust has a fresh position for Last.fm scrobble timing.
    const now = Date.now();
    const key = title + '|' + artist + '|' + album + '|' + playing + '|' + volume;
    if (key === last && !(playing && now - lastEmit > 10000)) return;
    last = key;
    lastEmit = now;
    try {
      window.__TAURI__.event.emit('media-update', { title, artist, album, art, playing, pos, dur, volume });
    } catch (e) {}
  }

  setInterval(poll, 1000);
  setInterval(enforceVolumeCurve, 250); // fast enough to catch song switches
})();
