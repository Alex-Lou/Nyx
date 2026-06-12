// Sprint 4.6 — mode lecture : heuristique JS auto-contenue (pas de dépendance),
// extrait le bloc d'article principal et le réaffiche dans un gabarit Nyx.
// Re-cliquer recharge la page pour en sortir.

use gtk::gio;
use webkit2gtk::{WebView, WebViewExt};

const READER_JS: &str = r#"
(function () {
    if (document.getElementById('nyx-reader')) { location.reload(); return; }

    var best = null, bestLen = 0;
    document.querySelectorAll('article, main, [role="main"], [itemprop="articleBody"]')
        .forEach(function (el) {
            var len = (el.innerText || '').length;
            if (len > bestLen) { best = el; bestLen = len; }
        });

    if (!best) {
        // Fallback : le parent qui cumule le plus de texte de <p>
        var totals = new Map();
        Array.prototype.forEach.call(document.getElementsByTagName('p'), function (p) {
            var parent = p.parentElement;
            if (!parent) return;
            var len = (totals.get(parent) || 0) + (p.innerText || '').length;
            totals.set(parent, len);
            if (len > bestLen) { best = parent; bestLen = len; }
        });
    }
    if (!best || bestLen < 400) return; // pas un article

    var title = document.title || '';
    var content = best.innerHTML;

    document.documentElement.innerHTML = '<head><meta charset="utf-8"></head><body></body>';
    document.title = title;

    var style = document.createElement('style');
    style.textContent =
        'body{background:#0a0a18;color:#dde4ff;font:18px/1.7 Georgia,serif;margin:0;padding:48px 16px}' +
        '#nyx-reader{max-width:680px;margin:0 auto}' +
        '#nyx-reader h1{font-size:30px;line-height:1.3;color:#c8d6ff}' +
        '#nyx-reader h2,#nyx-reader h3{color:#c8d6ff}' +
        '#nyx-reader img,#nyx-reader video{max-width:100%;height:auto}' +
        '#nyx-reader a{color:#4af2c8}' +
        '#nyx-reader pre{overflow-x:auto;background:#0f0f22;padding:12px;border-radius:8px;font-size:14px}' +
        '#nyx-reader blockquote{border-left:3px solid #7b8cde;margin-left:0;padding-left:16px;color:#7078a8}';
    document.head.appendChild(style);

    var div = document.createElement('div');
    div.id = 'nyx-reader';
    var h1 = document.createElement('h1');
    h1.textContent = title;
    div.appendChild(h1);
    var article = document.createElement('div');
    article.innerHTML = content;
    div.appendChild(article);
    div.querySelectorAll('script,iframe,form,button,nav,aside,svg')
        .forEach(function (n) { n.remove(); });
    document.body.appendChild(div);
})();
"#;

/// Bascule la page courante en mode lecture (ou en sort via reload).
pub fn toggle(webview: &WebView) {
    webview.evaluate_javascript(
        READER_JS,
        None,
        None,
        gio::Cancellable::NONE,
        |_| {},
    );
}
