//! Anti-pub YouTube (et trackers cosmétiques) — Sprint 3, complément réseau.
//!
//! Les pubs vidéo YouTube sont servies depuis le même domaine que la vidéo
//! (googlevideo.com) : impossible à filtrer côté réseau sans casser la
//! lecture. La parade (celle d'uBlock Origin) est un UserScript injecté qui
//! masque les pubs display et *skippe* les pubs in-stream dès qu'elles
//! démarrent. Ciblé par allow-list sur les domaines YouTube uniquement.

use webkit2gtk::{
    UserContentInjectedFrames, UserContentManagerExt, UserScript,
    UserScriptInjectionTime, WebView, WebViewExt,
};

const YOUTUBE_JS: &str = r#"
(function () {
    'use strict';

    // 1. Masquer les pubs display (bannières, encarts, in-feed, shorts).
    var css = [
        '.ytp-ad-overlay-slot', '.ytp-ad-overlay-container', '.ytp-ad-message-container',
        '#player-ads', '#masthead-ad', 'ytd-promoted-sparkles-web-renderer',
        'ytd-display-ad-renderer', 'ytd-ad-slot-renderer', 'ytd-in-feed-ad-layout-renderer',
        'ytd-promoted-video-renderer', '.ytd-companion-slot-renderer',
        'ytd-engagement-panel-section-list-renderer[target-id="engagement-panel-ads"]',
        'ad-slot-renderer', 'ytm-promoted-sparkles-web-renderer'
    ].join(',') + '{display:none !important}';
    var style = document.createElement('style');
    style.textContent = css;
    (document.head || document.documentElement).appendChild(style);

    // 2. Skipper les pubs in-stream : clic « Passer » dès qu'il apparaît,
    //    sinon on saute à la fin de la pub (lecture muette accélérée).
    function killStreamAd() {
        var player = document.querySelector('.html5-video-player');
        var video  = document.querySelector('video');
        if (!player || !video) return;

        if (player.classList.contains('ad-showing') || player.classList.contains('ad-interrupting')) {
            var skip = document.querySelector(
                '.ytp-ad-skip-button, .ytp-ad-skip-button-modern, .ytp-skip-ad-button, ' +
                '.ytp-ad-skip-button-container button'
            );
            if (skip) {
                skip.click();
            } else if (isFinite(video.duration) && video.duration > 0) {
                video.muted = true;
                video.currentTime = video.duration;   // saute la pub non-skippable
            }
        }
    }

    // Boucle légère + observation des mutations (YouTube est une SPA).
    setInterval(killStreamAd, 250);
    var mo = new MutationObserver(killStreamAd);
    if (document.body) {
        mo.observe(document.body, { childList: true, subtree: true });
    } else {
        document.addEventListener('DOMContentLoaded', function () {
            mo.observe(document.body, { childList: true, subtree: true });
        });
    }
})();
"#;

/// Patterns d'allow-list (domaines YouTube uniquement).
const YOUTUBE_HOSTS: &[&str] = &[
    "*://*.youtube.com/*",
    "*://youtube.com/*",
    "*://*.youtube-nocookie.com/*",
    "*://m.youtube.com/*",
];

/// Branche l'anti-pub YouTube sur une WebView (une fois par WebView).
pub fn wire(webview: &WebView) {
    let Some(ucm) = webview.user_content_manager() else { return };
    ucm.add_script(&UserScript::new(
        YOUTUBE_JS,
        UserContentInjectedFrames::AllFrames, // couvre aussi les embeds
        UserScriptInjectionTime::Start,
        YOUTUBE_HOSTS,
        &[],
    ));
}
