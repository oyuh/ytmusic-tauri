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
  // loud. We intercept the `volume` setter on HTMLMediaElement itself: every write YT
  // makes is curved to x^2 on the way through, so there is never a moment where the
  // element sits at the raw linear value. The getter hands back the linear value YT
  // asked for, so the slider, getVolume, and the Stream Deck all still agree on 0-100.
  // This runs as an init script (document start), so it's in place before YT creates
  // any <video>, and it covers every element YT swaps in on a track change.
  const VOL_EXP = 2;
  const desc = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'volume');
  Object.defineProperty(HTMLMediaElement.prototype, 'volume', {
    configurable: true,
    enumerable: desc.enumerable,
    get() {
      return this.__ytmVol !== undefined ? this.__ytmVol : desc.get.call(this);
    },
    set(x) {
      const lin = Math.max(0, Math.min(1, Number(x) || 0));
      this.__ytmVol = lin;
      desc.set.call(this, Math.pow(lin, VOL_EXP));
    },
  });

  // A brand-new media element starts at a raw 1.0, and YT doesn't always write a volume
  // to it before starting playback - that gap is the split-second of full blast on a
  // track change. Seed any element we haven't seen from the player's own volume before
  // it can make a sound. Media events don't bubble, so listen in the capture phase to
  // catch them from every element without watching the DOM.
  function seedVolume(e) {
    const v = e.target;
    if (!(v instanceof HTMLMediaElement) || v.__ytmVol !== undefined) return;
    const p = document.querySelector('#movie_player');
    let lin = 1;
    try { if (p && p.getVolume) lin = Math.max(0, Math.min(100, p.getVolume())) / 100; } catch (err) {}
    v.volume = lin; // through the setter above, so it lands curved
  }
  document.addEventListener('loadstart', seedVolume, true);
  document.addEventListener('play', seedVolume, true);

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
})();
