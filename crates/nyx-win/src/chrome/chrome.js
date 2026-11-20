// Chrome Nyx (Windows) — vue pure : n'a AUCUNE logique de navigation.
// Envoie des commandes JSON à Rust (window.ipc) et affiche l'état reçu via
// window.nyx.update(). Tout texte reçu passe par textContent / value
// (jamais innerHTML). Les passphrases saisies ici partent vers Rust puis
// les champs sont vidés aussitôt ; elles ne sont jamais stockées côté JS.
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
    const star = $('star');
    const progress = $('progress');
    const toastEl = $('toast');
    const prompt = $('prompt');

    // Écran du coffre.
    const lock = $('lock');
    const pass = $('pass');
    const confirm = $('confirm');
    const capsuleBox = $('capsule');
    const capsuleOpt = $('capsule-opt');
    const capsuleImport = $('capsule-import');
    const capsulePass = $('capsule-pass');
    const capsuleMsg = $('capsule-msg');
    const capsuleCmd = $('capsule-cmd');
    const capsuleRetry = $('capsule-retry');
    const lockError = $('lock-error');
    const lockOk = $('lock-ok');
    const lockCancel = $('lock-cancel');

    let shownUrl = '';
    let loading = false;
    let toastTimer = 0;
    let promptId = 0;
    let overlay = null;
    let capsuleCommand = '';

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

    function clearSecrets() {
        pass.value = '';
        confirm.value = '';
        capsulePass.value = '';
    }

    const CAPSULE_MSG = {
        probing: 'Recherche de la capsule Nyx (WSL)…',
        found: '',
        missing: 'Aucun coffre Nyx dans la capsule.',
        unreachable: 'Capsule injoignable : WSL est arrêté. Démarre-la dans un terminal, puis Réessayer :',
    };

    // (Re)dessine l'écran du coffre ; `fresh` = vient d'apparaître.
    function renderLock(fresh) {
        if (!overlay) {
            lock.hidden = true;
            clearSecrets();
            return;
        }
        lock.hidden = false;
        const unlock = overlay.kind === 'unlock';
        const creating = unlock && overlay.creating;
        const capsule = overlay.capsule;
        const found = capsule === 'found';

        $('lock-title').textContent = creating
            ? 'Crée la passphrase de ton coffre Nyx'
            : unlock ? 'Coffre Nyx verrouillé' : 'Importer depuis la capsule Nyx (Linux)';
        $('lock-sub').textContent = creating
            ? 'Elle chiffre tes mots de passe, favoris et historique. Irrécupérable si oubliée.'
            : unlock
                ? 'Saisis ta passphrase pour déverrouiller mots de passe, favoris et historique.'
                : 'Mots de passe, favoris, historique et réglages du coffre WSL. Le coffre source n\'est jamais modifié.';

        pass.hidden = !unlock;
        pass.placeholder = 'Passphrase';
        confirm.hidden = !creating;

        capsuleBox.hidden = !(creating || !unlock);
        capsuleOpt.hidden = !(creating && found);
        capsulePass.hidden = !found || (creating && !capsuleImport.checked);
        capsulePass.placeholder = creating ? 'Passphrase de la capsule (vide = la même)' : 'Passphrase de la capsule';
        capsuleMsg.textContent = CAPSULE_MSG[capsule] || '';
        capsuleMsg.hidden = !capsuleMsg.textContent;
        capsuleCmd.textContent = capsuleCommand;
        capsuleCmd.hidden = capsule !== 'unreachable';
        capsuleRetry.hidden = !(capsule === 'unreachable' || capsule === 'missing');

        lockOk.textContent = creating ? 'Créer le coffre' : unlock ? 'Déverrouiller' : 'Importer';
        lockOk.disabled = !unlock && !found;
        lockCancel.textContent = unlock ? 'Quitter' : 'Annuler';
        lockError.textContent = String(overlay.error || '');

        if (fresh || overlay.error) (unlock ? pass : capsulePass).focus();
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

            star.disabled = !s.canBookmark;
            star.classList.toggle('on', Boolean(s.bookmarked));
            star.textContent = s.bookmarked ? '★' : '☆';
            star.title = s.bookmarked ? 'Retirer des favoris' : 'Ajouter aux favoris';
            star.setAttribute('aria-pressed', String(Boolean(s.bookmarked)));

            promptId = s.prompt ? s.prompt.id : 0;
            prompt.hidden = !s.prompt;
            $('prompt-text').textContent = s.prompt ? String(s.prompt.text) : '';

            const fresh = Boolean(s.overlay) && (!overlay || overlay.kind !== s.overlay.kind);
            overlay = s.overlay || null;
            capsuleCommand = String(s.capsuleCmd || '');
            renderLock(fresh);
        },
        toast(text) {
            toastEl.textContent = String(text);
            toastEl.classList.add('on');
            clearTimeout(toastTimer);
            toastTimer = setTimeout(() => toastEl.classList.remove('on'), 5000);
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
    star.addEventListener('click', () => send('toggleBookmark'));
    document.querySelectorAll('[data-open]').forEach((btn) =>
        btn.addEventListener('click', () => send('navigate', { text: btn.dataset.open })));

    $('prompt-yes').addEventListener('click', () => promptId && send('promptAnswer', { id: promptId, yes: true }));
    $('prompt-no').addEventListener('click', () => promptId && send('promptAnswer', { id: promptId, yes: false }));

    $('lock-form').addEventListener('submit', (e) => {
        e.preventDefault();
        if (!overlay) return;
        lockError.textContent = '';
        if (overlay.kind === 'unlock' && overlay.creating) {
            send('create', {
                passphrase: pass.value,
                confirm: confirm.value,
                importCapsule: overlay.capsule === 'found' && capsuleImport.checked,
                capsulePassphrase: capsulePass.value,
            });
        } else if (overlay.kind === 'unlock') {
            send('unlock', { passphrase: pass.value });
        } else {
            send('importCapsule', { passphrase: capsulePass.value });
        }
        clearSecrets();
    });
    lockCancel.addEventListener('click', () => send(overlay && overlay.kind === 'unlock' ? 'quit' : 'closeOverlay'));
    capsuleRetry.addEventListener('click', () => send('retryCapsule'));
    capsuleImport.addEventListener('change', () => renderLock(false));
    lock.addEventListener('keydown', (e) => {
        if (e.key === 'Escape' && overlay && overlay.kind !== 'unlock') send('closeOverlay');
    });

    document.addEventListener('contextmenu', (e) => {
        if (e.target !== url && !(e.target instanceof HTMLInputElement)) e.preventDefault();
    });

    send('ready');
})();
