// Chrome Nyx (Windows) — vue pure : n'a AUCUNE logique de navigation.
// Envoie des commandes JSON à Rust (window.ipc) et affiche l'état reçu via
// window.nyx.update(). Tout texte reçu passe par textContent / value
// (jamais innerHTML).
'use strict';
(() => {
    const $ = (id) => document.getElementById(id);
    const send = (cmd, extra) => {
        if (window.ipc) window.ipc.postMessage(JSON.stringify(Object.assign({ cmd }, extra)));
    };

    const tabs = $('tabs');
    const url = $('url');
    const back = $('back');
    const forward = $('forward');
    const reload = $('reload');
    const progress = $('progress');
    const toastEl = $('toast');

    let shownUrl = '';
    let loading = false;
    let toastTimer = 0;

    function tabElement(t) {
        const el = document.createElement('div');
        el.className = 'tab' + (t.active ? ' active' : '') + (t.loading ? ' loading' : '');
        el.setAttribute('role', 'tab');
        el.setAttribute('aria-selected', String(t.active));
        el.title = t.title;

        const dot = document.createElement('span');
        dot.className = 'dot';
        const label = document.createElement('span');
        label.className = 'label';
        label.textContent = t.title;
        const close = document.createElement('button');
        close.className = 'close';
        close.textContent = '×';
        close.title = "Fermer l'onglet (Ctrl+W)";
        close.addEventListener('click', (e) => {
            e.stopPropagation();
            send('closeTab', { id: t.id });
        });

        el.addEventListener('mousedown', (e) => {
            if (e.button === 0) send('selectTab', { id: t.id });
        });
        el.addEventListener('auxclick', (e) => {
            if (e.button === 1) send('closeTab', { id: t.id });
        });
        el.append(dot, label, close);
        return el;
    }

    function setLoading(on) {
        loading = on;
        reload.textContent = on ? '✕' : '⟳';
        reload.title = on ? 'Arrêter (Échap)' : 'Recharger (F5)';
        progress.classList.toggle('on', on);
    }

    window.nyx = Object.freeze({
        update(s) {
            if (!s || !Array.isArray(s.tabs)) return;
            tabs.replaceChildren(...s.tabs.map(tabElement));
            shownUrl = String(s.url || '');
            if (document.activeElement !== url) url.value = shownUrl;
            back.disabled = !s.canGoBack;
            forward.disabled = !s.canGoForward;
            setLoading(Boolean(s.loading));
        },
        toast(text) {
            toastEl.textContent = String(text);
            toastEl.classList.add('on');
            clearTimeout(toastTimer);
            toastTimer = setTimeout(() => toastEl.classList.remove('on'), 4000);
        },
        focusUrl() {
            url.focus();
            url.select();
        },
    });

    $('go').addEventListener('submit', (e) => {
        e.preventDefault();
        const text = url.value.trim();
        if (!text) return;
        send('navigate', { text });
        url.blur();
    });
    url.addEventListener('focus', () => url.select());
    url.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') {
            url.value = shownUrl;
            url.blur();
        }
    });
    url.addEventListener('blur', () => {
        if (!url.value.trim()) url.value = shownUrl;
    });

    back.addEventListener('click', () => send('back'));
    forward.addEventListener('click', () => send('forward'));
    reload.addEventListener('click', () => send(loading ? 'stop' : 'reload'));
    $('home').addEventListener('click', () => send('home'));
    $('newtab').addEventListener('click', () => send('newTab'));
    $('downloads').addEventListener('click', () => send('downloads'));
    document.addEventListener('contextmenu', (e) => {
        if (e.target !== url) e.preventDefault();
    });

    send('ready');
})();
