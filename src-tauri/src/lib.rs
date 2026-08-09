use std::cell::RefCell;
use std::time::Duration;

use serde::Deserialize;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::utils::config::Color;
use tauri::{Listener, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_updater::UpdaterExt;

// Check GitHub Releases for a newer version and install it silently, then restart.
// Runs in the background on startup so it never blocks the window.
async fn auto_update(app: tauri::AppHandle) {
    let Ok(updater) = app.updater() else { return };
    match updater.check().await {
        Ok(Some(update)) => {
            if update.download_and_install(|_, _| {}, || {}).await.is_ok() {
                app.restart();
            }
        }
        _ => {}
    }
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

// souvlaki's MediaControls is not Send on Windows, so it lives on the main thread.
// Metadata updates are marshalled here via AppHandle::run_on_main_thread.
thread_local! {
    static CONTROLS: RefCell<Option<MediaControls>> = RefCell::new(None);
}

#[derive(Deserialize)]
struct MediaState {
    title: String,
    artist: String,
    album: String,
    art: String,
    playing: bool,
    pos: f64,
    dur: f64,
}

// JS to run a method on YT Music's player element for an OS media-control event.
fn player_js(method: &str) -> String {
    format!("(function(){{var p=document.querySelector('#movie_player');if(p&&p.{m})p.{m}();}})()", m = method)
}

// --- Local control server (for the Stream Deck plugin) ---
// A tiny localhost HTTP server that maps GET paths to player actions. Bound to
// 127.0.0.1 only. ponytail: no auth — any local process can reach it; fine for a
// personal app, revisit with a token if that ever matters. Fixed port.
const CONTROL_PORT: u16 = 7897;

fn set_volume_js(expr: &str) -> String {
    format!("(function(){{var p=document.querySelector('#movie_player');if(p&&p.setVolume)p.setVolume({});}})()", expr)
}

fn route_js(url: &str) -> Option<String> {
    let path = url.split('?').next().unwrap_or("");
    match path {
        "/playpause" => Some("(function(){var p=document.querySelector('#movie_player');if(p){p.getPlayerState()===1?p.pauseVideo():p.playVideo();}})()".to_string()),
        "/play" => Some(player_js("playVideo")),
        "/pause" => Some(player_js("pauseVideo")),
        "/next" => Some(player_js("nextVideo")),
        "/prev" | "/previous" => Some(player_js("previousVideo")),
        "/volup" => Some(set_volume_js("Math.min(100,(p.getVolume()||0)+5)")),
        "/voldown" => Some(set_volume_js("Math.max(0,(p.getVolume()||0)-5)")),
        "/volume" => {
            let v: i32 = url
                .split('?')
                .nth(1)?
                .split('&')
                .find_map(|kv| kv.strip_prefix("v="))?
                .parse()
                .ok()?;
            Some(set_volume_js(&format!("Math.max(0,Math.min(100,{}))", v.clamp(0, 100))))
        }
        _ => None,
    }
}

fn start_control_server(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let server = match tiny_http::Server::http(("127.0.0.1", CONTROL_PORT)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("control server failed to start on {CONTROL_PORT}: {e}");
                return;
            }
        };
        for req in server.incoming_requests() {
            if let Some(js) = route_js(req.url()) {
                if let Some(wv) = app.get_webview_window("main") {
                    let _ = wv.eval(&js);
                }
            }
            let mut resp = tiny_http::Response::from_string("ok");
            if let Ok(h) =
                tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..])
            {
                resp.add_header(h);
            }
            let _ = req.respond(resp);
        }
    });
}

pub fn run() {
    tauri::Builder::default()
        // Must be the FIRST plugin: a second launch just focuses the running instance
        // (bringing it back from the tray) instead of opening a duplicate window.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let window = WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::External("https://music.youtube.com".parse().unwrap()),
            )
            .title("YT Music")
            .inner_size(1100.0, 780.0)
            .min_inner_size(480.0, 360.0)
            .decorations(false)
            // Near-black webview background (matches YT Music) so there's no white
            // flash before the page paints on launch.
            .background_color(Color(3, 3, 3, 255))
            // Allow programmatic playback (needed for the Stream Deck "play" command) and
            // disable WebView2's built-in SMTC (HardwareMediaKeyHandling) so our souvlaki
            // session is the only one — otherwise Windows shows a duplicate "WebView2" entry.
            .additional_browser_args(
                "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,HardwareMediaKeyHandling --autoplay-policy=no-user-gesture-required",
            )
            .initialization_script(include_str!("../titlebar.js"))
            .initialization_script(include_str!("../media.js"))
            .build()?;

            // Register the Windows media session under our own name/icon.
            let hwnd = window.hwnd()?.0 as *mut std::ffi::c_void;
            let config = PlatformConfig {
                display_name: "YT Music",
                dbus_name: "ytmusic",
                hwnd: Some(hwnd),
            };
            let mut controls = MediaControls::new(config)
                .map_err(|e| format!("failed to create media controls: {:?}", e))?;

            // Route OS control events (media keys / the Windows flyout buttons) back to
            // YT Music's player.
            let app_ctrl = app.handle().clone();
            controls
                .attach(move |event: MediaControlEvent| {
                    let js = match event {
                        MediaControlEvent::Play => player_js("playVideo"),
                        MediaControlEvent::Pause | MediaControlEvent::Stop => player_js("pauseVideo"),
                        MediaControlEvent::Next => player_js("nextVideo"),
                        MediaControlEvent::Previous => player_js("previousVideo"),
                        MediaControlEvent::Toggle => "(function(){var p=document.querySelector('#movie_player');if(p){p.getPlayerState()===1?p.pauseVideo():p.playVideo();}})()".to_string(),
                        _ => return,
                    };
                    if let Some(wv) = app_ctrl.get_webview_window("main") {
                        let _ = wv.eval(&js);
                    }
                })
                .map_err(|e| format!("failed to attach media controls: {:?}", e))?;

            CONTROLS.with(|c| *c.borrow_mut() = Some(controls));

            // Receive playback state from the page and push it to the OS media session.
            let app_listen = app.handle().clone();
            app.listen("media-update", move |event| {
                let state: MediaState = match serde_json::from_str(event.payload()) {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let _ = app_listen.run_on_main_thread(move || {
                    CONTROLS.with(|c| {
                        let mut c = c.borrow_mut();
                        let Some(controls) = c.as_mut() else { return };
                        let _ = controls.set_metadata(MediaMetadata {
                            title: Some(&state.title),
                            artist: Some(&state.artist),
                            album: if state.album.is_empty() { None } else { Some(&state.album) },
                            cover_url: if state.art.is_empty() { None } else { Some(&state.art) },
                            duration: if state.dur > 0.0 { Some(Duration::from_secs_f64(state.dur)) } else { None },
                        });
                        let progress = Some(MediaPosition(Duration::from_secs_f64(state.pos.max(0.0))));
                        let playback = if state.playing {
                            MediaPlayback::Playing { progress }
                        } else {
                            MediaPlayback::Paused { progress }
                        };
                        let _ = controls.set_playback(playback);
                    });
                });
            });

            start_control_server(app.handle().clone());

            // System tray: the minimize button hides the window (leaves the taskbar);
            // the tray icon / menu brings it back. Left-click shows, menu has Show/Quit.
            let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("YT Music")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;

            tauri::async_runtime::spawn(auto_update(app.handle().clone()));

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::route_js;

    #[test]
    fn routes_map_correctly() {
        assert!(route_js("/next").unwrap().contains("nextVideo"));
        assert!(route_js("/prev").unwrap().contains("previousVideo"));
        assert!(route_js("/playpause").unwrap().contains("pauseVideo"));
        assert!(route_js("/volup").unwrap().contains("setVolume"));
        assert!(route_js("/voldown").unwrap().contains("setVolume"));
        assert!(route_js("/volume?v=42").unwrap().contains("42"));
        assert_eq!(route_js("/volume"), None); // missing v
        assert_eq!(route_js("/bogus"), None);
    }
}
