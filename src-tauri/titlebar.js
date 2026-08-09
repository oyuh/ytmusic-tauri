// Injected into every YT Music page (top frame only). Turns the empty parts of
// YT Music's own nav bar into a window drag region and adds min/max/close buttons
// styled to match, so there's no separate/ugly OS title bar.
//
// YT Music enforces Trusted Types + a strict CSP, so we must NOT use innerHTML or
// injected <style> blocks: SVGs are built with createElementNS and every style is
// set via CSSOM (element.style), both of which the page's policy allows.
(function () {
  if (window.top !== window.self) return; // skip iframes

  const NS = 'http://www.w3.org/2000/svg';
  const win = () => window.__TAURI__ && window.__TAURI__.window.getCurrentWindow();

  function icon(children) {
    const s = document.createElementNS(NS, 'svg');
    s.setAttribute('width', '10');
    s.setAttribute('height', '10');
    s.setAttribute('viewBox', '0 0 10 10');
    for (const [tag, attrs] of children) {
      const el = document.createElementNS(NS, tag);
      for (const k in attrs) el.setAttribute(k, attrs[k]);
      s.appendChild(el);
    }
    return s;
  }

  const ICONS = {
    minimize: () => icon([['rect', { x: 0, y: 4.5, width: 10, height: 1, fill: 'currentColor' }]]),
    maximize: () => icon([['rect', { x: 0.5, y: 0.5, width: 9, height: 9, fill: 'none', stroke: 'currentColor' }]]),
    close: () => icon([['path', { d: 'M0 0 L10 10 M10 0 L0 10', stroke: 'currentColor' }]]),
  };

  function button(label, onclick) {
    const b = document.createElement('button');
    b.type = 'button';
    b.setAttribute('aria-label', label);
    b.appendChild(ICONS[label]());
    Object.assign(b.style, {
      width: '46px', height: '40px', display: 'flex', alignItems: 'center',
      justifyContent: 'center', background: 'transparent', border: '0',
      color: '#ccc', cursor: 'pointer', borderRadius: '4px', padding: '0',
      transition: 'background .1s, color .1s',
    });
    const hoverBg = label === 'close' ? '#e81123' : 'rgba(255,255,255,.12)';
    b.addEventListener('mouseenter', () => { b.style.background = hoverBg; b.style.color = '#fff'; });
    b.addEventListener('mouseleave', () => { b.style.background = 'transparent'; b.style.color = '#ccc'; });
    b.addEventListener('click', onclick);
    return b;
  }

  function addControls(nav) {
    const right = nav.querySelector('#right-content');
    if (!right) return;

    // Drag region: the empty flanks of the nav bar. data-tauri-drag-region only
    // starts a drag when the click target IS the flagged element, so the search
    // box and icon buttons inside these areas stay clickable.
    nav.querySelectorAll('.center-content, .left-content, #right-content').forEach(el => {
      el.setAttribute('data-tauri-drag-region', '');
    });

    // Reserve space at the right end of the nav so YT Music's own icons never sit
    // under the (fixed) window controls. Padding the #right-content element shifts
    // its cast/avatar icons left regardless of the nav's responsive flex/abs layout.
    const RESERVE = '150px';
    if (right.style.paddingRight !== RESERVE) right.style.paddingRight = RESERVE;

    if (document.querySelector('.ytm-winctl')) return; // controls already added

    // Fixed to the window's top-right corner so the controls are always visible and
    // at the corner, independent of YT Music's (responsive) nav layout.
    const box = document.createElement('div');
    box.className = 'ytm-winctl';
    Object.assign(box.style, {
      position: 'fixed', top: '0', right: '0', height: '64px',
      display: 'flex', alignItems: 'center', zIndex: '2147483647',
    });
    // Minimize hides to the system tray (off the taskbar) instead of a normal minimize.
    box.appendChild(button('minimize', () => win() && win().hide()));
    box.appendChild(button('maximize', () => win() && win().toggleMaximize()));
    box.appendChild(button('close', () => win() && win().close()));
    document.body.appendChild(box);
  }

  // Replace WebView2's chunky default scrollbar (arrows + wide track) with a thin,
  // sleek one. Uses the standard scrollbar-width/scrollbar-color properties set via
  // CSSOM (not a <style> block), so it isn't subject to the page's CSP. These are
  // inherited, so YT Music's inner scroll areas get the same sleek bar.
  function styleScrollbars() {
    const de = document.documentElement;
    if (!de || de.dataset.ytmScroll) return;
    de.style.scrollbarWidth = 'thin';
    de.style.scrollbarColor = 'rgba(255,255,255,.25) transparent';
    de.dataset.ytmScroll = '1';
  }

  const tryInject = () => {
    styleScrollbars();
    const nav = document.querySelector('ytmusic-nav-bar');
    if (nav) addControls(nav);
  };

  // Poll instead of MutationObserver: the init script runs at document-creation,
  // when document.documentElement may not exist yet (observing it throws). A 300ms
  // querySelector is negligible and keeps working across SPA re-renders.
  // ponytail: 300ms poll; switch to an observer only if this ever shows up in a profile.
  setInterval(tryInject, 300);
})();
