// Capture des soumissions de mots de passe — partagé desktop (WebKitGTK,
// UserScript cadre principal), Windows (WebView2, script d'initialisation
// cadre principal) et Android (WebView, injecté en fin de chargement). Écoute submit / clic / Entrée (SPA, login en deux temps)
// et poste {nyx:'password', username, password, host} à Nyx, qui valide
// l'origine de l'émetteur puis demande Oui/Non avant tout enregistrement.
(function () {
    if (window.top !== window) return; // jamais dans une iframe
    var KEY = '__nyx_last_user';
    var USER_RE = /user|login|email|mail|identifiant|account|compte|phone/i;
    var BTN_RE = /log ?in|sign ?in|connexion|se connecter|continuer|continue|next|suivant/i;
    var posted = {};

    // Canal natif : WebKitGTK (message handler dédié), WebView2, ou Android
    // (objet `nyx_password` de WebViewCompat.addWebMessageListener, injecté
    // seulement dans les cadres https).
    function post(msg) {
        var json = JSON.stringify(msg);
        var wk = window.webkit && window.webkit.messageHandlers;
        if (wk && wk.nyx_password) {
            wk.nyx_password.postMessage(json);
        } else if (window.chrome && window.chrome.webview) {
            window.chrome.webview.postMessage(json);
        } else if (window.nyx_password && typeof window.nyx_password.postMessage === 'function') {
            window.nyx_password.postMessage(json);
        }
    }

    function visible(el) {
        return !!(el && (el.offsetWidth || el.offsetHeight || el.getClientRects().length));
    }
    function lastUser() {
        try { return sessionStorage.getItem(KEY) || ''; } catch (_) { return ''; }
    }
    function remember(v) {
        try { if (v) sessionStorage.setItem(KEY, v); } catch (_) {}
    }
    function formOf(el) {
        return el && el.closest ? el.closest('form') : null;
    }

    // Mot de passe à envoyer : dernier champ non vide (inscription / changement).
    function findPassword(root) {
        var all = Array.prototype.filter.call(
            root.querySelectorAll('input[type="password"]'),
            function (p) { return p.value; });
        return all.length ? all[all.length - 1] : null;
    }

    // Champ identifiant : autocomplete, email, nom évocateur, sinon le champ
    // texte rempli le plus proche avant le mot de passe.
    function pickUser(root, pwd) {
        var inputs = Array.prototype.filter.call(
            root.querySelectorAll('input'),
            function (i) { return i !== pwd && i.type !== 'password' && i.value && visible(i); });
        var ac = function (i) { return (i.getAttribute('autocomplete') || '').toLowerCase(); };
        var textual = function (i) {
            return i.type === 'text' || i.type === 'tel' || i.type === 'email' || i.type === 'search';
        };
        return inputs.filter(function (i) { return /\b(username|email)\b/.test(ac(i)); })[0]
            || inputs.filter(function (i) { return i.type === 'email'; })[0]
            || inputs.filter(function (i) {
                return (i.type === 'text' || i.type === 'tel') &&
                    USER_RE.test(i.name + ' ' + i.id + ' ' + ac(i));
            })[0]
            || (pwd && inputs.filter(function (i) {
                return textual(i) &&
                    (i.compareDocumentPosition(pwd) & Node.DOCUMENT_POSITION_FOLLOWING);
            }).pop())
            || null;
    }

    function findUser(scope, pwd) {
        var form = (pwd && pwd.form) || formOf(scope);
        return (form && pickUser(form, pwd)) || pickUser(document, pwd);
    }

    // Déclencheur (submit / clic / Entrée) : poste si un mot de passe est
    // rempli, sinon mémorise l'identifiant (première étape d'un login en deux temps).
    function handle(scope) {
        var form = formOf(scope);
        var pwd = (form && findPassword(form)) || findPassword(document);
        var user = findUser(scope, pwd);
        if (user) remember(user.value);
        if (!pwd) return;
        var name = user ? user.value : lastUser();
        var key = name + '\u0000' + pwd.value;
        if (posted[key]) return;
        posted[key] = true;
        post({
            nyx: 'password',
            username: name,
            password: pwd.value,
            host: location.hostname
        });
    }

    function submitLike(el) {
        if (!el) return false;
        var t = (el.type || '').toLowerCase();
        if (el.tagName === 'INPUT' && t === 'submit') return true;
        if (el.tagName === 'BUTTON' && t === 'submit') return true;
        if (el.tagName === 'BUTTON' && !el.getAttribute('type') && el.form) return true;
        var txt = [el.textContent, el.getAttribute('aria-label'), el.id, el.name, el.value].join(' ');
        return BTN_RE.test(txt.slice(0, 200));
    }

    document.addEventListener('submit', function (e) {
        if (e.target && e.target.querySelector) handle(e.target);
    }, true);

    document.addEventListener('click', function (e) {
        var el = e.target && e.target.closest
            ? e.target.closest('button, input[type="submit"], input[type="button"], a, [role="button"]')
            : null;
        if (submitLike(el)) handle(el);
    }, true);

    document.addEventListener('keydown', function (e) {
        var t = e.target;
        if (e.key === 'Enter' && t && t.tagName === 'INPUT' && (t.type === 'password' || !findPassword(document))) {
            handle(t);
        }
    }, true);
})();
