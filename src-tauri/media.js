// Media bridge: reports what YT Music is playing to the Rust side, which mirrors it
// to Windows SMTC (the media popup) under the "YT Music" identity. WebView2's own
// SMTC session is disabled (see additional_browser_args) so there's no duplicate.
//
// Reads from YT Music's #movie_player (always present) rather than only
// navigator.mediaSession, so it's robust. Emits only when the track or play/pause
// state changes, not every tick.
(function () {
  if (window.top !== window.self) return;

  // Perceptual volume curve. YT Music maps its slider linearly to amplitude, so low
  // numbers are still loud. Remap so the number you set stays the same but actual
  // loudness follows (n/100)^2 (gentler than Spotify's ~cube so the low end doesn't
  // drop to silence). Crucially we also feed the number back through YT's native
  // setVolume so YT's own stored volume stays in sync, otherwise YT periodically
  // re-applies its old value and Stream Deck / slider changes snap back.
  function installVolumeCurve() {
    const p = document.querySelector('#movie_player');
    const v = document.querySelector('video');
    if (!p || !v || !p.setVolume || p.__ytmVolCurve) return;
    p.__ytmVolCurve = true;
    const EXP = 2;
    const rawSet = p.setVolume.bind(p);
    const clamp = (n) => Math.max(0, Math.min(100, Math.round(n)));
    let number = clamp(p.getVolume());
    const want = () => Math.pow(number / 100, EXP);
    p.setVolume = (d) => { number = clamp(d); rawSet(number); v.volume = want(); };
    p.getVolume = () => number;
    v.addEventListener('volumechange', () => {
      if (Math.abs(v.volume - want()) < 0.0006) return; // our own curved set
      number = clamp(v.volume * 100); // external change (slider / keyboard)
      v.volume = want();
    });
    v.volume = want();
  }

  let last = '';
  let lastEmit = 0;
  function poll() {
    installVolumeCurve();
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
