//! Rejet automatique des bannières de consentement cookies (nouveau #1).
//!
//! Approche « Consent-O-Matic light », 100 % local : un UserScript injecté
//! au plus tôt qui (1) masque les bandeaux de consentement connus, et
//! (2) clique le bouton « Refuser / Tout refuser / Nécessaires seulement »
//! dès qu'il apparaît — souvent avant même que la bannière soit peinte.
//!
//! Le blocage réseau des cookies tiers est, lui, assuré par la politique
//! `CookieAcceptPolicy::NoThirdParty` (voir main.rs) — ceci s'attaque à la
//! couche UI/JS que le réseau ne voit pas. Activable dans les réglages.

use webkit2gtk::{
    UserContentInjectedFrames, UserContentManagerExt, UserScript,
    UserScriptInjectionTime, WebView, WebViewExt,
};

const COOKIE_JS: &str = r#"
(function () {
    'use strict';

    // Mots-clés « refuser » multilingues (FR/EN/ES/DE/IT/PT).
    var REJECT = [
        'tout refuser', 'refuser tout', 'refuser', 'continuer sans accepter',
        'reject all', 'reject', 'decline', 'necessary only', 'only necessary',
        'rechazar', 'ablehnen', 'rifiuta', 'recusar', 'nur notwendige'
    ];
    // CMP connus → masquage immédiat (avant peinture).
    var HIDE = [
        '#onetrust-banner-sdk', '#onetrust-consent-sdk', '.ot-sdk-container',
        '#CybotCookiebotDialog', '#cookiebanner', '.cookie-banner', '.cookie-consent',
        '#didomi-host', '.didomi-popup-container', '#cmpbox', '.cmpboxBG',
        '#usercentrics-root', '.qc-cmp2-container', '#cookie-law-info-bar',
        '.cc-window', '.cookie-notice', '#gdpr-consent-tool-wrapper', '#sp_message_container_'
    ];

    var style = document.createElement('style');
    style.textContent = HIDE.join(',') + '{display:none !important;visibility:hidden !important}'
        + 'html.nyx-no-cookie-scroll-lock{overflow:auto !important}';
    (document.head || document.documentElement).appendChild(style);

    function norm(s) { return (s || '').trim().toLowerCase().replace(/\s+/g, ' '); }

    function clickReject() {
        var nodes = document.querySelectorAll('button, a, [role="button"], input[type="button"], input[type="submit"]');
        for (var i = 0; i < nodes.length; i++) {
            var el = nodes[i];
            var txt = norm(el.innerText || el.textContent || el.value || el.getAttribute('aria-label'));
            if (!txt || txt.length > 40) continue;
            for (var j = 0; j < REJECT.length; j++) {
                if (txt === REJECT[j] || txt.indexOf(REJECT[j]) !== -1) {
                    el.click();
                    // Le site peut verrouiller le scroll : on le rétablit.
                    document.documentElement.classList.add('nyx-no-cookie-scroll-lock');
                    document.documentElement.style.overflow = 'auto';
                    if (document.body) document.body.style.overflow = 'auto';
                    return true;
                }
            }
        }
        return false;
    }

    // Tente tout de suite, puis sur chaque mutation (bannières tardives),
    // avec un garde-fou temporel pour ne pas tourner indéfiniment.
    clickReject();
    var tries = 0;
    var mo = new MutationObserver(function () {
        if (clickReject() || ++tries > 40) { mo.disconnect(); }
    });
    function start() { if (document.body) mo.observe(document.body, { childList: true, subtree: true }); }
    if (document.body) start(); else document.addEventListener('DOMContentLoaded', start);
})();
"#;

/// Branche le rejet de cookies sur une WebView (gardé par le réglage).
pub fn wire(webview: &WebView) {
    let Some(ucm) = webview.user_content_manager() else { return };
    ucm.add_script(&UserScript::new(
        COOKIE_JS,
        UserContentInjectedFrames::TopFrame,
        UserScriptInjectionTime::Start,
        &[],
        &["nyx://*", "file://*"], // jamais sur nos pages internes
    ));
}
