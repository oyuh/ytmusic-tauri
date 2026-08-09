# YT Music

A tiny, fast desktop wrapper for [music.youtube.com](https://music.youtube.com). Windows only.

It's basically the website in a stripped-down window but it shows up as real media in Windows, minimizes to the tray, and updates itself. There's also a little Stream Deck plugin for controlling it.

## Install

Grab the latest `YT Music_x.x.x_x64-setup.exe` from [Releases](https://github.com/oyuh/ytmusic-tauri/releases) and run it. That's it. It'll auto-update itself after that, so you only install once.

## What it does

- Loads YT Music in a ~10 MB native window (WebView2), opens fast
- Window controls live in YT Music's own top bar no ugly OS title bar
- Now-playing shows in the Windows media flyout + hardware media keys work
- **Minimize** hides it to the system tray (relaunching just brings that one back). **Close** quits.
- Scrobbles to Last.fm (optional, YT Music's own scrobbling is flaky)
- Checks for updates on launch and installs them silently

## Last.fm scrobbling

YT Music's own scrobbling is unreliable, so the app scrobbles for you. One-time setup:

1. **Make a Last.fm API app** (free, instant) at [last.fm/api/account/create](https://www.last.fm/api/account/create). Give it any name, leave the **Callback URL** blank, and submit. You'll get an **API key** and a **Shared secret**, keep that page open.
2. **Right-click the app's tray icon** (near the clock, maybe under the `^` overflow) and pick **Connect Last.fm**. A setup page opens in your browser.
3. **Paste the API key + shared secret** into the two boxes and click **Connect**. A Last.fm tab pops up.
4. On that tab, click **Yes, allow access**. Then go back to the setup page and click **Finish**. It should say "Connected as <you>".

That's it. From then on it scrobbles automatically: it sets "now playing" when a track starts and scrobbles it once you're halfway through (or 4 minutes in, whichever comes first, per Last.fm's rules).

Your key, secret, and session live in `%APPDATA%\com.lawson.ytmusic\lastfm.json` on your machine only, never in this repo. To disconnect, delete that file. To reconnect on a new track after setup, nothing to do, it just works while the app is running.

## Stream Deck plugin

Optional. Buttons for play/pause, previous, next, volume up/down. The play/pause key shows the current state, and the volume keys show the current level.

Copy `streamdeck/com.lawson.ytmusic.sdPlugin` into `%APPDATA%\Elgato\StreamDeck\Plugins\`, then fully restart Stream Deck. A **YT Music** category shows up, drag the actions onto keys. They talk to the app over `127.0.0.1:7897`, so it just works while the app is running.

## Dev

```bash
npm install
npm run tauri dev
```

## Releases

Push a `v*` tag and GitHub Actions builds the Windows installer, signs the update, and publishes a release:

```bash
npm version patch   # bumps version, or edit it by hand
git tag v0.1.1 && git push --tags
```

Needs two repo secrets for update signing: `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
