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
    const passOld = $('pass-old');
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
    const lockAlt = $('lock-alt');
    const lockCancel = $('lock-cancel');

    let shownUrl = '';
    let loading = false;
    let toastTimer = 0;
    let promptId = 0;
    let overlay = null;
    let capsuleCommand = '';
    // Bienvenue : champs du mot de passe principal révélés ?
    let defining = false;

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
        passOld.value = '';
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

    const WELCOME_SUB =
        'Nyx chiffre toujours tes mots de passe, favoris et historique. Par défaut, la clé est gardée ' +
        'par ta session Windows : rien à retenir, mais toute personne ayant accès à ta session ouverte ' +
        'peut lire les mots de passe enregistrés (comme dans Chrome). Un mot de passe principal, ' +
        'optionnel, les protège aussi dans ce cas — mais il est irrécupérable si tu l\'oublies. ' +
        'Tu pourras le définir plus tard dans les Paramètres.';

    // Panneaux « mot de passe principal » (réglages). old : mot de passe
    // actuel demandé ; fresh : nouveau + confirmation demandés.
    const MASTER = {
        define: {
            title: 'Définir un mot de passe principal',
            sub: 'Il sera demandé à chaque lancement de Nyx et protège tes données même si quelqu\'un ' +
                'accède à ta session ouverte. Irrécupérable si oublié.',
            ok: 'Définir', old: false, fresh: true,
        },
        change: {
            title: 'Changer le mot de passe principal',
            sub: 'Saisis l\'actuel, puis le nouveau.',
            ok: 'Changer', old: true, fresh: true,
        },
        remove: {
            title: 'Retirer le mot de passe principal',
            sub: 'Nyx ouvrira le coffre sans rien demander, avec une clé gardée par ta session Windows. ' +
                'Toute personne ayant accès à ta session ouverte pourra lire les mots de passe enregistrés.',
            ok: 'Retirer', old: true, fresh: false,
        },
    };

    // (Re)dessine l'écran du coffre ; `fresh` = vient d'apparaître.
    function renderLock(fresh) {
        if (!overlay) {
            lock.hidden = true;
            defining = false;
            clearSecrets();
            return;
        }
        lock.hidden = false;
        const kind = overlay.kind;
        const welcome = kind === 'welcome';
        const unlock = kind === 'unlock';
        const importing = kind === 'capsule';
        const m = kind === 'master' ? (MASTER[overlay.mode] || MASTER.define) : null;
        const capsule = overlay.capsule;
        const found = capsule === 'found';
        if (fresh && welcome) defining = false;

        $('lock-title').textContent = welcome ? 'Bienvenue dans Nyx'
            : unlock ? 'Coffre Nyx verrouillé'
                : importing ? 'Importer depuis la capsule Nyx (Linux)' : m.title;
        $('lock-sub').textContent = welcome ? WELCOME_SUB
            : unlock ? String(overlay.notice || '') ||
                'Saisis ton mot de passe principal pour déverrouiller mots de passe, favoris et historique.'
                : importing
                    ? 'Mots de passe, favoris, historique et réglages du coffre WSL. Le coffre source n\'est jamais modifié.'
                    : m.sub;

        passOld.hidden = !(m && m.old);
        pass.hidden = !(unlock || (welcome && defining) || (m && m.fresh));
        pass.placeholder = unlock ? 'Mot de passe principal' : 'Nouveau mot de passe (8 caractères min.)';
        confirm.hidden = !((welcome && defining) || (m && m.fresh));

        capsuleBox.hidden = !(welcome || importing);
        capsuleOpt.hidden = !(welcome && found);
        capsulePass.hidden = !found || (welcome && !capsuleImport.checked);
        capsulePass.placeholder = 'Passphrase de la capsule (vide = clé de la capsule)';
        capsuleMsg.textContent = CAPSULE_MSG[capsule] || '';
        capsuleMsg.hidden = !capsuleMsg.textContent;
        capsuleCmd.textContent = capsuleCommand;
        capsuleCmd.hidden = capsule !== 'unreachable';
        capsuleRetry.hidden = !(capsule === 'unreachable' || capsule === 'missing');

        // Bienvenue : « Plus tard » est l'action principale tant que le mot
        // de passe n'est pas demandé ; Entrée ne crée jamais un coffre
        // sans mot de passe pendant qu'on en tape un.
        lockOk.textContent = welcome ? (defining ? 'Créer avec ce mot de passe' : 'Plus tard')
            : unlock ? 'Déverrouiller' : importing ? 'Importer' : m.ok;
        lockOk.disabled = importing && !found;
        lockOk.classList.toggle('danger', Boolean(m) && overlay.mode === 'remove');
        lockAlt.hidden = !welcome;
        lockAlt.textContent = defining ? 'Plus tard' : 'Définir un mot de passe principal';
        lockCancel.textContent = welcome || unlock ? 'Quitter' : 'Annuler';
        lockError.textContent = String(overlay.error || '');

        if (fresh || overlay.error) {
            const first = [passOld, pass].find((el) => !el.hidden) || (importing ? capsulePass : lockOk);
            first.focus();
        }
    }

    function createVault(master) {
        const extra = master ? { passphrase: pass.value, confirm: confirm.value } : {};
        send('create', Object.assign({
            master,
            importCapsule: overlay.capsule === 'found' && capsuleImport.checked,
            capsulePassphrase: capsulePass.value,
        }, extra));
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

            const fresh = Boolean(s.overlay) &&
                (!overlay || overlay.kind !== s.overlay.kind || overlay.mode !== s.overlay.mode);
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
        const kind = overlay.kind;
        if (kind === 'welcome') {
            createVault(defining);
        } else if (kind === 'unlock') {
            send('unlock', { passphrase: pass.value });
        } else if (kind === 'capsule') {
            send('importCapsule', { passphrase: capsulePass.value });
        } else if (kind === 'master' && overlay.mode === 'define') {
            send('setMaster', { passphrase: pass.value, confirm: confirm.value });
        } else if (kind === 'master' && overlay.mode === 'change') {
            send('changeMaster', { old: passOld.value, passphrase: pass.value, confirm: confirm.value });
        } else if (kind === 'master' && overlay.mode === 'remove') {
            send('removeMaster', { passphrase: passOld.value });
        }
        clearSecrets();
    });
    lockAlt.addEventListener('click', () => {
        if (!overlay || overlay.kind !== 'welcome') return;
        if (!defining) {
            defining = true;
            renderLock(false);
            pass.focus();
        } else {
            lockError.textContent = '';
            createVault(false);
            clearSecrets();
        }
    });
    const lockedScreen = () => Boolean(overlay) && (overlay.kind === 'welcome' || overlay.kind === 'unlock');
    lockCancel.addEventListener('click', () => send(lockedScreen() ? 'quit' : 'closeOverlay'));
    capsuleRetry.addEventListener('click', () => send('retryCapsule'));
    capsuleImport.addEventListener('change', () => renderLock(false));
    lock.addEventListener('keydown', (e) => {
        if (e.key === 'Escape' && overlay && !lockedScreen()) send('closeOverlay');
    });

    document.addEventListener('contextmenu', (e) => {
        if (e.target !== url && !(e.target instanceof HTMLInputElement)) e.preventDefault();
    });

    send('ready');
})();
