// Media bridge: reports what YT Music is playing to the Rust side, which mirrors it
// to Windows SMTC (the media popup) under the "YT Music" identity. WebView2's own
// SMTC session is disabled (see additional_browser_args) so there's no duplicate.
//
// Reads from YT Music's #movie_player (always present) rather than only
// navigator.mediaSession, so it's robust. Emits only when the track or play/pause
// state changes, not every tick.
(function () {
  if (window.top !== window.self) return;

  let last = '';
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

    let pos = 0, dur = 0;
    try { pos = p.getCurrentTime() || 0; dur = p.getDuration() || 0; } catch (e) {}

    const key = title + '|' + artist + '|' + playing;
    if (key === last) return;
    last = key;
    try {
      window.__TAURI__.event.emit('media-update', { title, artist, album: '', art, playing, pos, dur });
    } catch (e) {}
  }
  setInterval(poll, 1000);
})();
