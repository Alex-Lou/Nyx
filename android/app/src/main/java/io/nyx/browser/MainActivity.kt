package io.nyx.browser

import android.animation.Animator
import android.animation.AnimatorListenerAdapter
import android.animation.AnimatorSet
import android.animation.ObjectAnimator
import android.Manifest
import android.app.Activity
import android.app.DownloadManager
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.content.pm.ActivityInfo
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.text.TextUtils
import android.util.Log
import android.view.Gravity
import android.view.KeyEvent
import android.view.Menu
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.view.animation.DecelerateInterpolator
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.webkit.CookieManager
import android.webkit.GeolocationPermissions
import android.webkit.PermissionRequest
import android.webkit.RenderProcessGoneDetail
import android.webkit.URLUtil
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.ImageButton
import android.widget.LinearLayout
import android.widget.PopupMenu
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import androidx.activity.OnBackPressedCallback
import androidx.activity.result.ActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import androidx.core.splashscreen.SplashScreen
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.core.widget.doAfterTextChanged
import androidx.webkit.WebSettingsCompat
import androidx.webkit.WebViewAssetLoader
import androidx.webkit.WebViewFeature
import com.google.android.material.bottomsheet.BottomSheetDialog
import com.google.android.material.button.MaterialButton
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import java.io.ByteArrayInputStream
import java.net.URISyntaxException

class MainActivity : AppCompatActivity() {

    private val tabs = mutableListOf<BrowserTab>()
    private var current: BrowserTab? = null

    private lateinit var root: View
    private lateinit var container: FrameLayout
    private lateinit var fullscreen: FrameLayout
    private lateinit var address: EditText
    private lateinit var btnBack: ImageButton
    private lateinit var btnForward: ImageButton
    private lateinit var btnReload: ImageButton
    private lateinit var btnTabs: TextView
    private lateinit var btnMenu: ImageButton
    private lateinit var progress: ProgressBar
    private lateinit var findBar: View
    private lateinit var findInput: EditText
    private lateinit var findCount: TextView
    private lateinit var assetLoader: WebViewAssetLoader

    /** Coffre : capture + pré-remplissage des mots de passe (cf. [PasswordCapture]). */
    private lateinit var capture: PasswordCapture

    /** Favoris / Historique / Mots de passe : l'URL choisie s'ouvre dans l'onglet courant. */
    private val libraryLauncher =
        registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
            val url = result.data?.getStringExtra(LibraryActivity.EXTRA_URL)
            if (result.resultCode == Activity.RESULT_OK && url != null) openFromLibrary(url)
        }

    /** Suite d'une demande de permissions Android en cours (une seule à la fois). */
    private var pendingPermissions: (() -> Unit)? = null
    private val permissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
            // Résultat relu via checkSelfPermission : seule source de vérité.
            val next = pendingPermissions
            pendingPermissions = null
            next?.invoke()
        }

    /** Origines autorisées à lire du contenu protégé (DRM), pour la session. */
    private val drmOrigins = mutableSetOf<String>()

    /** Vidéo plein écran : vue fournie par la WebView de [customViewTab]. */
    private var customView: View? = null
    private var customViewCallback: WebChromeClient.CustomViewCallback? = null
    private var customViewTab: BrowserTab? = null

    /** Envoi de fichiers (`<input type=file>`) : un seul sélecteur ouvert à la fois. */
    private var fileCallback: ValueCallback<Array<Uri>>? = null
    private var fileCallbackTab: BrowserTab? = null
    private val fileChooser =
        registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
            deliverFiles(result)
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        val splash = installSplashScreen()
        super.onCreate(savedInstanceState)
        setupSplashExit(splash)

        WindowCompat.setDecorFitsSystemWindows(window, false)
        setContentView(R.layout.activity_main)

        root = findViewById(R.id.root)
        container = findViewById(R.id.web_container)
        fullscreen = findViewById(R.id.fullscreen)
        address = findViewById(R.id.address)
        btnBack = findViewById(R.id.btn_back)
        btnForward = findViewById(R.id.btn_forward)
        btnReload = findViewById(R.id.btn_reload)
        btnTabs = findViewById(R.id.btn_tabs)
        btnMenu = findViewById(R.id.btn_menu)
        progress = findViewById(R.id.progress)
        findBar = findViewById(R.id.find_bar)
        findInput = findViewById(R.id.find_input)
        findCount = findViewById(R.id.find_count)

        WindowInsetsControllerCompat(window, window.decorView).apply {
            isAppearanceLightStatusBars = false
            isAppearanceLightNavigationBars = false
        }
        ViewCompat.setOnApplyWindowInsetsListener(root) { v, insets ->
            val bars = insets.getInsets(
                WindowInsetsCompat.Type.systemBars() or
                    WindowInsetsCompat.Type.displayCutout() or
                    WindowInsetsCompat.Type.ime()
            )
            v.setPadding(bars.left, bars.top, bars.right, bars.bottom)
            WindowInsetsCompat.CONSUMED
        }

        assetLoader = NewTabPage.loader(this)
        capture = PasswordCapture(this)
        bindChrome()

        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() = onBack()
        })

        openTab(viewIntentUrl(intent))
        // Coffre : ouverture silencieuse (clé d'appareil), sinon bienvenue
        // ou mot de passe principal. La navigation n'attend pas.
        VaultDialogs.start(this) {
            current?.let { capture.onPageFinished(it.webView, it.pageUrl) }
            // Appareils liés : synchronisation à l'ouverture (Wi-Fi seulement).
            SyncRepo.autoSync(this)
        }
    }

    // ── Splash ─────────────────────────────────────────────────────────────

    /** Sortie du splash : icône agrandie + fondu, 300 ms, décélérée. */
    private fun setupSplashExit(splash: SplashScreen) {
        splash.setOnExitAnimationListener { provider ->
            if (reducedMotion()) {
                provider.remove()
                return@setOnExitAnimationListener
            }
            try {
                val icon = provider.iconView
                val set = AnimatorSet()
                set.playTogether(
                    ObjectAnimator.ofFloat(icon, View.SCALE_X, 1f, 1.35f),
                    ObjectAnimator.ofFloat(icon, View.SCALE_Y, 1f, 1.35f),
                    ObjectAnimator.ofFloat(icon, View.ALPHA, 1f, 0f),
                    ObjectAnimator.ofFloat(provider.view, View.ALPHA, 1f, 0f),
                )
                set.duration = 300L
                set.interpolator = DecelerateInterpolator()
                set.addListener(object : AnimatorListenerAdapter() {
                    override fun onAnimationEnd(animation: Animator) {
                        provider.remove()
                    }
                })
                set.start()
            } catch (e: RuntimeException) {
                // Pas d'icône (lancement hors launcher sur API 31+…) : sortie sèche.
                provider.remove()
            }
        }
    }

    private fun reducedMotion(): Boolean =
        Settings.Global.getFloat(contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f

    // ── Barre d'outils ─────────────────────────────────────────────────────

    private fun bindChrome() {
        btnBack.setOnClickListener { current?.webView?.takeIf { it.canGoBack() }?.goBack() }
        btnForward.setOnClickListener { current?.webView?.takeIf { it.canGoForward() }?.goForward() }
        btnReload.setOnClickListener { current?.webView?.reload() }
        btnTabs.setOnClickListener { showTabSwitcher() }
        btnTabs.setOnLongClickListener {
            showQuickMenu(it)
            true
        }
        btnMenu.setOnClickListener { showMainMenu(it) }
        bindFindBar()

        address.setOnEditorActionListener { _, actionId, event ->
            val enter = event != null &&
                event.keyCode == KeyEvent.KEYCODE_ENTER &&
                event.action == KeyEvent.ACTION_DOWN
            if (actionId == EditorInfo.IME_ACTION_GO || enter) {
                submitAddress()
                true
            } else {
                false
            }
        }
        address.setOnFocusChangeListener { _, hasFocus ->
            if (!hasFocus) refreshAddress()
        }
    }

    private fun submitAddress() {
        val tab = current ?: return
        val resolved = NyxCore.safeResolveInput(address.text.toString())
        if (resolved.isEmpty()) {
            Toast.makeText(this, R.string.address_refused, Toast.LENGTH_SHORT).show()
            return
        }
        hideKeyboard()
        address.clearFocus()
        load(tab, resolved)
        tab.webView.requestFocus()
    }

    /** `nyx://…` → page Nouvel onglet ; le reste est déjà validé http(s). */
    private fun load(tab: BrowserTab, url: String) {
        val target = if (url.startsWith("nyx://")) NewTabPage.URL else url
        tab.webView.loadUrl(target)
    }

    private fun hideKeyboard() {
        val imm = getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
        imm.hideSoftInputFromWindow(address.windowToken, 0)
    }

    /** Appui long sur le compteur d'onglets. */
    private fun showQuickMenu(anchor: View) {
        val menu = PopupMenu(this, anchor)
        menu.menu.add(Menu.NONE, MENU_FIND, Menu.NONE, R.string.find_in_page)
        menu.menu.add(Menu.NONE, MENU_NEW_TAB, Menu.NONE, R.string.new_tab)
        menu.setOnMenuItemClickListener { item ->
            when (item.itemId) {
                MENU_FIND -> openFindBar()
                MENU_NEW_TAB -> openTab(null)
            }
            true
        }
        menu.show()
    }

    /** Menu principal (bouton ⋮) : favori, bibliothèque, réglages. */
    private fun showMainMenu(anchor: View) {
        val menu = PopupMenu(this, anchor)
        val url = current?.pageUrl.orEmpty()
        val canStar = bookmarkable(url)
        val starred = canStar && VaultRepo.bookmarkId(url) >= 0
        val star = menu.menu.add(
            Menu.NONE, MENU_BOOKMARK, Menu.NONE,
            if (starred) R.string.bookmark_remove else R.string.bookmark_add,
        )
        star.isEnabled = canStar
        menu.menu.add(Menu.NONE, MENU_NEW_TAB, Menu.NONE, R.string.new_tab)
        menu.menu.add(Menu.NONE, MENU_FIND, Menu.NONE, R.string.find_in_page)
        menu.menu.add(Menu.NONE, MENU_BOOKMARKS, Menu.NONE, R.string.bookmarks_title)
        menu.menu.add(Menu.NONE, MENU_HISTORY, Menu.NONE, R.string.history_title)
        menu.menu.add(Menu.NONE, MENU_PASSWORDS, Menu.NONE, R.string.passwords_title)
        menu.menu.add(Menu.NONE, MENU_SETTINGS, Menu.NONE, R.string.settings_title)
        menu.setOnMenuItemClickListener { item ->
            when (item.itemId) {
                MENU_BOOKMARK -> toggleBookmark()
                MENU_NEW_TAB -> openTab(null)
                MENU_FIND -> openFindBar()
                MENU_BOOKMARKS -> openLibrary(LibraryActivity.MODE_BOOKMARKS)
                MENU_HISTORY -> openLibrary(LibraryActivity.MODE_HISTORY)
                MENU_PASSWORDS -> openLibrary(LibraryActivity.MODE_PASSWORDS)
                MENU_SETTINGS -> startActivity(Intent(this, SettingsActivity::class.java))
            }
            true
        }
        menu.show()
    }

    private fun bookmarkable(url: String): Boolean =
        (url.startsWith("https://") || url.startsWith("http://")) && !NewTabPage.isNewTab(url)

    /** Étoile : ajoute ou retire la page courante des favoris (coffre). */
    private fun toggleBookmark() {
        val tab = current ?: return
        val url = tab.pageUrl
        if (!VaultRepo.isOpen) {
            Toast.makeText(this, R.string.vault_locked, Toast.LENGTH_SHORT).show()
            return
        }
        if (!bookmarkable(url)) return
        val id = VaultRepo.bookmarkId(url)
        val message = if (id >= 0) {
            if (NyxVault.bookmarkDelete(id)) R.string.bookmark_removed else R.string.bookmark_failed
        } else {
            if (NyxVault.bookmarkAdd(url, tab.title) >= 0) R.string.bookmark_added else R.string.bookmark_failed
        }
        Toast.makeText(this, message, Toast.LENGTH_SHORT).show()
    }

    private fun openLibrary(mode: String) {
        libraryLauncher.launch(Intent(this, LibraryActivity::class.java).putExtra(LibraryActivity.EXTRA_MODE, mode))
    }

    private fun openFromLibrary(url: String) {
        val scheme = Uri.parse(url).scheme?.lowercase()
        if (scheme != "http" && scheme != "https") return
        val tab = current
        if (tab == null) openTab(url) else load(tab, url)
    }

    // ── Rechercher dans la page ────────────────────────────────────────────

    private fun bindFindBar() {
        findInput.doAfterTextChanged { text ->
            if (findBar.visibility != View.VISIBLE) return@doAfterTextChanged
            val query = text?.toString().orEmpty()
            val wv = current?.webView ?: return@doAfterTextChanged
            if (query.isEmpty()) {
                wv.clearMatches()
                findCount.text = ""
            } else {
                wv.findAllAsync(query)
            }
        }
        findInput.setOnEditorActionListener { _, actionId, event ->
            val enter = event != null &&
                event.keyCode == KeyEvent.KEYCODE_ENTER &&
                event.action == KeyEvent.ACTION_DOWN
            if (actionId == EditorInfo.IME_ACTION_SEARCH || enter) {
                findStep(forward = true)
                true
            } else {
                false
            }
        }
        findViewById<View>(R.id.find_prev).setOnClickListener { findStep(forward = false) }
        findViewById<View>(R.id.find_next).setOnClickListener { findStep(forward = true) }
        findViewById<View>(R.id.find_close).setOnClickListener { closeFindBar() }
    }

    private fun openFindBar() {
        if (current == null) return
        findBar.visibility = View.VISIBLE
        findCount.text = ""
        findInput.requestFocus()
        findInput.selectAll()
        val imm = getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
        imm.showSoftInput(findInput, InputMethodManager.SHOW_IMPLICIT)
        // Relance la recherche si un texte est resté dans le champ.
        val query = findInput.text.toString()
        if (query.isNotEmpty()) current?.webView?.findAllAsync(query)
    }

    private fun findStep(forward: Boolean) {
        val wv = current?.webView ?: return
        if (findInput.text.isNullOrEmpty()) return
        wv.findNext(forward)
    }

    /** Ferme la barre et efface les surlignages de l'onglet courant. */
    private fun closeFindBar() {
        if (findBar.visibility != View.VISIBLE) return
        findBar.visibility = View.GONE
        findCount.text = ""
        val imm = getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
        imm.hideSoftInputFromWindow(findInput.windowToken, 0)
        findInput.clearFocus()
        current?.webView?.clearMatches()
    }

    private fun onFindResult(tab: BrowserTab, activeMatchOrdinal: Int, numberOfMatches: Int) {
        if (tab !== current || findBar.visibility != View.VISIBLE) return
        findCount.text = if (numberOfMatches == 0) {
            if (findInput.text.isNullOrEmpty()) "" else "0/0"
        } else {
            getString(R.string.find_count, activeMatchOrdinal + 1, numberOfMatches)
        }
    }

    private fun refreshAddress() {
        val tab = current ?: return
        if (address.hasFocus()) return
        val url = tab.pageUrl
        address.setText(if (NewTabPage.isNewTab(url)) "" else url)
    }

    private fun refreshNav() {
        val wv = current?.webView ?: return
        btnBack.isEnabled = wv.canGoBack()
        btnBack.alpha = if (btnBack.isEnabled) 1f else 0.35f
        btnForward.isEnabled = wv.canGoForward()
        btnForward.alpha = if (btnForward.isEnabled) 1f else 0.35f
    }

    private fun refreshProgress() {
        val p = current?.progress ?: 100
        progress.progress = p
        progress.visibility = if (p in 1..99) View.VISIBLE else View.INVISIBLE
    }

    private fun refreshTabCount() {
        btnTabs.text = if (tabs.size > 99) ":D" else tabs.size.toString()
    }

    private fun refreshChrome() {
        refreshAddress()
        refreshNav()
        refreshProgress()
        refreshTabCount()
    }

    // ── Onglets ────────────────────────────────────────────────────────────

    private fun openTab(url: String?) {
        val webView = WebView(this)
        val tab = BrowserTab(webView)
        configureWebView(webView)
        webView.webViewClient = TabClient(tab)
        webView.webChromeClient = TabChromeClient(tab)
        webView.setDownloadListener { link, userAgent, contentDisposition, mimeType, _ ->
            startDownload(link, userAgent, contentDisposition, mimeType)
        }
        webView.setFindListener { activeMatchOrdinal, numberOfMatches, _ ->
            onFindResult(tab, activeMatchOrdinal, numberOfMatches)
        }
        capture.attach(webView)
        tabs.add(tab)
        switchTo(tab)
        load(tab, url ?: VaultRepo.homeUrl().ifEmpty { NewTabPage.URL })
    }

    private fun switchTo(tab: BrowserTab) {
        val previous = current
        if (customViewTab != null && customViewTab !== tab) exitFullscreen(notifyPage = true)
        if (previous !== tab) closeFindBar()
        if (previous != null && previous !== tab) {
            previous.webView.onPause()
            container.removeView(previous.webView)
        }
        current = tab
        if (tab.webView.parent == null) {
            container.addView(
                tab.webView,
                FrameLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.MATCH_PARENT,
                ),
            )
        }
        tab.webView.onResume()
        refreshChrome()
    }

    private fun closeTab(tab: BrowserTab) {
        val index = tabs.indexOf(tab)
        if (index < 0) return
        tabs.removeAt(index)
        releaseTab(tab)
        if (current === tab) current = null
        destroyWebView(tab.webView)
        when {
            tabs.isEmpty() -> openTab(null)
            current == null -> switchTo(tabs[minOf(index, tabs.size - 1)])
            else -> refreshTabCount()
        }
    }

    /** Rend la main à la page : aucun callback WebView ne survit à l'onglet. */
    private fun releaseTab(tab: BrowserTab) {
        if (tab === current) closeFindBar()
        if (fileCallbackTab === tab) cancelFileChooser()
        if (customViewTab === tab) exitFullscreen(notifyPage = true)
        settlePagePermissions(tab)
    }

    /** Aucune fuite : détachée du parent, chargement arrêté, puis détruite. */
    private fun destroyWebView(webView: WebView) {
        (webView.parent as? ViewGroup)?.removeView(webView)
        webView.stopLoading()
        webView.setDownloadListener(null)
        webView.setFindListener(null)
        webView.webChromeClient = null
        webView.destroy()
    }

    private fun configureWebView(webView: WebView) {
        webView.setBackgroundColor(ContextCompat.getColor(this, R.color.nyx_void))
        val s = webView.settings
        s.javaScriptEnabled = true
        s.domStorageEnabled = true
        s.allowFileAccess = false
        s.allowContentAccess = false
        s.mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
        s.setSupportMultipleWindows(false)
        s.javaScriptCanOpenWindowsAutomatically = false
        s.mediaPlaybackRequiresUserGesture = true
        // Se présenter comme un vrai navigateur mobile : sans « ; wv » ni
        // « Version/4.0 », Google & co servent leur site mobile complet et
        // n'interdisent pas la connexion (refusée aux WebView intégrées).
        s.userAgentString = s.userAgentString
            .replace("; wv", "")
            .replace(Regex(" Version/\\d+(\\.\\d+)*"), "")
        s.loadWithOverviewMode = true
        s.useWideViewPort = true
        s.setSupportZoom(true)
        s.builtInZoomControls = true
        s.displayZoomControls = false
        // Safe Browsing : laissé à sa valeur par défaut (activé).
        if (WebViewFeature.isFeatureSupported(WebViewFeature.ALGORITHMIC_DARKENING)) {
            WebSettingsCompat.setAlgorithmicDarkeningAllowed(s, true)
        }
        CookieManager.getInstance().apply {
            setAcceptCookie(true)
            setAcceptThirdPartyCookies(webView, false)
        }
    }

    // ── Permissions Android ────────────────────────────────────────────────

    private fun isGranted(permission: String): Boolean =
        ContextCompat.checkSelfPermission(this, permission) == PackageManager.PERMISSION_GRANTED

    /**
     * Demande les permissions manquantes puis rend celles accordées. Une
     * seule demande à la fois : une seconde, concurrente, reçoit l'état actuel.
     */
    private fun withPermissions(permissions: Array<String>, onResult: (Set<String>) -> Unit) {
        val missing = permissions.filterNot { isGranted(it) }
        if (missing.isEmpty() || pendingPermissions != null) {
            onResult(permissions.filter { isGranted(it) }.toSet())
            return
        }
        pendingPermissions = { onResult(permissions.filter { isGranted(it) }.toSet()) }
        permissionLauncher.launch(missing.toTypedArray())
    }

    // ── Téléchargements ────────────────────────────────────────────────────

    /**
     * Confie un téléchargement http(s) au DownloadManager (dossier
     * Téléchargements, notification à la fin), avec les cookies et l'UA de la
     * page pour les liens authentifiés. Nom assaini par nyx-core.
     */
    private fun startDownload(url: String?, userAgent: String?, contentDisposition: String?, mimeType: String?) {
        val scheme = url?.let { Uri.parse(it).scheme?.lowercase() }
        if (url == null || (scheme != "http" && scheme != "https")) {
            // blob:/data: : il faudrait lire le contenu depuis la page (plus tard).
            Toast.makeText(this, R.string.download_unsupported, Toast.LENGTH_SHORT).show()
            return
        }
        val name = NyxCore.safeSanitizeFilename(URLUtil.guessFileName(url, contentDisposition, mimeType))
        val request = DownloadManager.Request(Uri.parse(url))
            .setTitle(name)
            .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)
        if (!mimeType.isNullOrBlank()) request.setMimeType(mimeType)
        CookieManager.getInstance().getCookie(url)?.let { request.addRequestHeader("Cookie", it) }
        if (!userAgent.isNullOrBlank()) request.addRequestHeader("User-Agent", userAgent)

        // API 29+ : écrire dans Téléchargements via DownloadManager ne demande rien.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            enqueueDownload(request, name)
            return
        }
        withPermissions(arrayOf(Manifest.permission.WRITE_EXTERNAL_STORAGE)) { granted ->
            if (Manifest.permission.WRITE_EXTERNAL_STORAGE in granted) {
                enqueueDownload(request, name)
            } else {
                Toast.makeText(this, R.string.download_no_storage, Toast.LENGTH_SHORT).show()
            }
        }
    }

    private fun enqueueDownload(request: DownloadManager.Request, name: String) {
        try {
            request.setDestinationInExternalPublicDir(Environment.DIRECTORY_DOWNLOADS, name)
            val dm = getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager
            dm.enqueue(request)
            Toast.makeText(this, getString(R.string.download_started, name), Toast.LENGTH_SHORT).show()
        } catch (e: RuntimeException) {
            // IllegalState (stockage indisponible), Security, IllegalArgument…
            Log.w(TAG, "Téléchargement refusé", e)
            Toast.makeText(this, R.string.download_failed, Toast.LENGTH_SHORT).show()
        }
    }

    // ── Permissions demandées par les pages ────────────────────────────────

    /**
     * Hôte d'une origine éligible aux permissions : https uniquement, jamais
     * la page Nouvel onglet interne. `null` = refus d'office.
     */
    private fun permissionHost(origin: Uri?): String? {
        if (origin == null || origin.scheme?.lowercase() != "https") return null
        val host = origin.host?.lowercase()
        return if (host.isNullOrEmpty() || host == NewTabPage.HOST) null else host
    }

    /** Ressource WebView → permission Android requise (`""` : aucune, `null` : refusée). */
    private fun androidPermissionFor(resource: String): String? = when (resource) {
        PermissionRequest.RESOURCE_VIDEO_CAPTURE -> Manifest.permission.CAMERA
        PermissionRequest.RESOURCE_AUDIO_CAPTURE -> Manifest.permission.RECORD_AUDIO
        PermissionRequest.RESOURCE_PROTECTED_MEDIA_ID -> ""
        else -> null
    }

    private fun resourceLabel(resource: String): String = when (resource) {
        PermissionRequest.RESOURCE_VIDEO_CAPTURE -> getString(R.string.perm_camera)
        PermissionRequest.RESOURCE_AUDIO_CAPTURE -> getString(R.string.perm_microphone)
        else -> getString(R.string.perm_drm)
    }

    private fun onPagePermissionRequest(tab: BrowserTab, request: PermissionRequest) {
        val host = permissionHost(request.origin)
        // Ressources inconnues (MIDI SysEx…) : jamais accordées.
        val wanted = request.resources.filter { androidPermissionFor(it) != null }
        if (host == null || wanted.isEmpty() || tab.permissionRequest != null || tab.prompt != null) {
            request.deny()
            return
        }
        val origin = request.origin.toString()
        if (wanted.all { it == PermissionRequest.RESOURCE_PROTECTED_MEDIA_ID } && origin in drmOrigins) {
            request.grant(wanted.toTypedArray())
            return
        }
        tab.permissionRequest = request
        val what = joinLabels(wanted.map { resourceLabel(it) })
        askUser(
            tab,
            getString(R.string.perm_message, host, what),
            onAllow = {
                val needed = wanted.mapNotNull { androidPermissionFor(it) }.filter { it.isNotEmpty() }
                withPermissions(needed.toTypedArray()) { granted ->
                    finishPermissionRequest(tab, request, origin, wanted, granted)
                }
            },
            onDeny = {
                if (tab.permissionRequest === request) {
                    tab.permissionRequest = null
                    request.deny()
                }
            },
        )
    }

    private fun finishPermissionRequest(
        tab: BrowserTab,
        request: PermissionRequest,
        origin: String,
        wanted: List<String>,
        granted: Set<String>,
    ) {
        // Onglet fermé ou demande annulée entre-temps : déjà soldée.
        if (tab.permissionRequest !== request) return
        tab.permissionRequest = null
        val allowed = wanted.filter {
            val permission = androidPermissionFor(it)
            permission != null && (permission.isEmpty() || permission in granted)
        }
        if (PermissionRequest.RESOURCE_PROTECTED_MEDIA_ID in allowed) drmOrigins.add(origin)
        if (allowed.isEmpty()) {
            request.deny()
        } else {
            request.grant(allowed.toTypedArray())
        }
        if (allowed.size < wanted.size) {
            Toast.makeText(this, R.string.perm_system_denied, Toast.LENGTH_SHORT).show()
        }
    }

    private fun cancelPagePermission(tab: BrowserTab, request: PermissionRequest) {
        if (tab.permissionRequest !== request) return
        tab.permissionRequest = null
        dismissPrompt(tab)
    }

    private fun onGeolocationPrompt(tab: BrowserTab, origin: String, callback: GeolocationPermissions.Callback) {
        val host = permissionHost(Uri.parse(origin))
        if (host == null || tab.geoCallback != null || tab.prompt != null) {
            callback.invoke(origin, false, false)
            return
        }
        tab.geoCallback = callback
        tab.geoOrigin = origin
        askUser(
            tab,
            getString(R.string.perm_message, host, getString(R.string.perm_location)),
            onAllow = {
                val location = arrayOf(
                    Manifest.permission.ACCESS_FINE_LOCATION,
                    Manifest.permission.ACCESS_COARSE_LOCATION,
                )
                withPermissions(location) { granted ->
                    finishGeolocation(tab, callback, granted.isNotEmpty())
                    if (granted.isEmpty()) {
                        Toast.makeText(this, R.string.perm_system_denied, Toast.LENGTH_SHORT).show()
                    }
                }
            },
            onDeny = { finishGeolocation(tab, callback, false) },
        )
    }

    /** `retain = false` : la décision ne vaut que pour cette demande. */
    private fun finishGeolocation(tab: BrowserTab, callback: GeolocationPermissions.Callback, allowed: Boolean) {
        if (tab.geoCallback !== callback) return
        val origin = tab.geoOrigin
        tab.geoCallback = null
        tab.geoOrigin = null
        if (origin != null) callback.invoke(origin, allowed, false)
    }

    private fun onGeolocationHidden(tab: BrowserTab) {
        val callback = tab.geoCallback ?: return
        finishGeolocation(tab, callback, false)
        dismissPrompt(tab)
    }

    /** Dialogue « <hôte> veut utiliser … » ; fermé sans choix = refus. */
    private fun askUser(tab: BrowserTab, message: String, onAllow: () -> Unit, onDeny: () -> Unit) {
        var allowed = false
        val dialog = MaterialAlertDialogBuilder(this)
            .setTitle(R.string.perm_title)
            .setMessage(message)
            .setPositiveButton(R.string.perm_allow) { _, _ -> allowed = true }
            .setNegativeButton(R.string.perm_deny, null)
            .create()
        dialog.setOnDismissListener {
            if (tab.prompt === dialog) tab.prompt = null
            if (allowed && tab in tabs) onAllow() else onDeny()
        }
        tab.prompt = dialog
        dialog.show()
    }

    private fun dismissPrompt(tab: BrowserTab) {
        val dialog = tab.prompt ?: return
        tab.prompt = null
        dialog.dismiss()
    }

    /** Fermeture d'onglet : tout ce qui est en attente est refusé. */
    private fun settlePagePermissions(tab: BrowserTab) {
        tab.permissionRequest?.let {
            tab.permissionRequest = null
            it.deny()
        }
        tab.geoCallback?.let { finishGeolocation(tab, it, false) }
        dismissPrompt(tab)
    }

    private fun joinLabels(labels: List<String>): String = when (labels.size) {
        0 -> ""
        1 -> labels[0]
        else -> labels.dropLast(1).joinToString(", ") + getString(R.string.perm_and) + labels.last()
    }

    // ── Plein écran ────────────────────────────────────────────────────────

    private fun enterFullscreen(tab: BrowserTab, view: View, callback: WebChromeClient.CustomViewCallback) {
        if (customView != null) {
            // Déjà en plein écran : on refuse la seconde vue.
            callback.onCustomViewHidden()
            return
        }
        hideKeyboard()
        customView = view
        customViewCallback = callback
        customViewTab = tab
        fullscreen.addView(
            view,
            FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT),
        )
        fullscreen.visibility = View.VISIBLE
        WindowInsetsControllerCompat(window, window.decorView).apply {
            systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            hide(WindowInsetsCompat.Type.systemBars())
        }
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_FULL_SENSOR
    }

    /**
     * Quitte le plein écran. `notifyPage` : sortie décidée par l'app (retour,
     * onglet fermé) — la page doit en être informée ; inutile quand c'est elle
     * qui l'a demandée (`onHideCustomView`).
     */
    private fun exitFullscreen(notifyPage: Boolean) {
        val view = customView ?: return
        val callback = customViewCallback
        customView = null
        customViewCallback = null
        customViewTab = null
        fullscreen.removeView(view)
        fullscreen.visibility = View.GONE
        WindowInsetsControllerCompat(window, window.decorView).show(WindowInsetsCompat.Type.systemBars())
        window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED
        if (notifyPage) callback?.onCustomViewHidden()
    }

    // ── Envoi de fichiers ──────────────────────────────────────────────────

    private fun showFileChooser(
        tab: BrowserTab,
        callback: ValueCallback<Array<Uri>>,
        params: WebChromeClient.FileChooserParams,
    ): Boolean {
        cancelFileChooser()
        val intent = try {
            params.createIntent()
        } catch (e: RuntimeException) {
            callback.onReceiveValue(null)
            return true
        }
        if (params.mode == WebChromeClient.FileChooserParams.MODE_OPEN_MULTIPLE) {
            intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
        }
        fileCallback = callback
        fileCallbackTab = tab
        try {
            fileChooser.launch(intent)
        } catch (e: ActivityNotFoundException) {
            cancelFileChooser()
            Toast.makeText(this, R.string.no_file_picker, Toast.LENGTH_SHORT).show()
        }
        return true
    }

    private fun deliverFiles(result: ActivityResult) {
        val callback = fileCallback ?: return
        fileCallback = null
        fileCallbackTab = null
        val data = result.data
        val clip = data?.clipData
        val uris: Array<Uri>? = if (result.resultCode == Activity.RESULT_OK && clip != null && clip.itemCount > 0) {
            // Sélection multiple : ClipData (parseResult ne lit que `data`).
            (0 until clip.itemCount).mapNotNull { clip.getItemAt(it).uri }.toTypedArray()
        } else {
            WebChromeClient.FileChooserParams.parseResult(result.resultCode, data)
        }
        callback.onReceiveValue(uris)
    }

    /** La page attend toujours une réponse : `null` = annulé. */
    private fun cancelFileChooser() {
        val callback = fileCallback ?: return
        fileCallback = null
        fileCallbackTab = null
        callback.onReceiveValue(null)
    }

    // ── Sélecteur d'onglets ────────────────────────────────────────────────

    private fun showTabSwitcher() {
        val dialog = BottomSheetDialog(this)
        val pad = dp(16)

        val list = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(pad, pad, pad, pad)
            setBackgroundColor(ContextCompat.getColor(this@MainActivity, R.color.nyx_surface))
        }

        val header = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        header.addView(
            TextView(this).apply {
                text = getString(R.string.tabs_title)
                textSize = 18f
                setTextColor(ContextCompat.getColor(this@MainActivity, R.color.nyx_text))
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        header.addView(
            MaterialButton(this).apply {
                text = getString(R.string.new_tab)
                setOnClickListener {
                    dialog.dismiss()
                    openTab(null)
                }
            },
        )
        list.addView(header)

        for (tab in tabs.toList()) {
            val row = LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                minimumHeight = dp(48)
            }
            val label = TextView(this).apply {
                text = tabLabel(tab)
                textSize = 15f
                maxLines = 1
                ellipsize = TextUtils.TruncateAt.END
                val color = if (tab === current) R.color.nyx_aurora else R.color.nyx_text
                setTextColor(ContextCompat.getColor(this@MainActivity, color))
                setPadding(0, dp(12), dp(8), dp(12))
                setOnClickListener {
                    dialog.dismiss()
                    switchTo(tab)
                }
            }
            row.addView(label, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            row.addView(
                ImageButton(this).apply {
                    setImageResource(R.drawable.ic_close)
                    contentDescription = getString(R.string.cd_close_tab)
                    setBackgroundResource(android.R.color.transparent)
                    setOnClickListener {
                        list.removeView(row)
                        val wasLast = tabs.size == 1
                        closeTab(tab)
                        if (wasLast) dialog.dismiss()
                    }
                },
                LinearLayout.LayoutParams(dp(44), dp(44)),
            )
            list.addView(row)
        }

        dialog.setContentView(ScrollView(this).apply { addView(list) })
        dialog.show()
    }

    private fun tabLabel(tab: BrowserTab): String = when {
        NewTabPage.isNewTab(tab.pageUrl) || tab.pageUrl.isEmpty() -> getString(R.string.new_tab)
        tab.title.isNotBlank() -> tab.title
        else -> tab.pageUrl
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    // ── Retour, intents, cycle de vie ─────────────────────────────────────

    private fun onBack() {
        if (customView != null) {
            exitFullscreen(notifyPage = true)
            return
        }
        if (findBar.visibility == View.VISIBLE) {
            closeFindBar()
            return
        }
        if (address.hasFocus()) {
            hideKeyboard()
            address.clearFocus()
            return
        }
        val tab = current ?: return finish()
        when {
            tab.webView.canGoBack() -> tab.webView.goBack()
            tabs.size > 1 -> closeTab(tab)
            else -> finish()
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        val url = viewIntentUrl(intent) ?: return
        openTab(url)
    }

    /** URL http(s) d'un intent VIEW (lien ouvert depuis une autre app). */
    private fun viewIntentUrl(intent: Intent?): String? {
        if (intent?.action != Intent.ACTION_VIEW) return null
        val url = intent.dataString ?: return null
        val scheme = Uri.parse(url).scheme?.lowercase()
        return if (scheme == "http" || scheme == "https") url else null
    }

    override fun onResume() {
        super.onResume()
        current?.webView?.onResume()
        // Retour dans l'app : synchronisation (Wi-Fi, appareil lié, ≤ 1/min).
        SyncRepo.autoSync(this)
    }

    override fun onPause() {
        current?.webView?.onPause()
        CookieManager.getInstance().flush()
        super.onPause()
    }

    override fun onDestroy() {
        capture.release()
        for (tab in tabs) {
            releaseTab(tab)
            destroyWebView(tab.webView)
        }
        tabs.clear()
        current = null
        super.onDestroy()
    }

    // ── Navigation externe ─────────────────────────────────────────────────

    /**
     * Confie `intent:`, `market:`, `tel:`, `mailto:` à Android. L'intent est
     * assaini (catégorie BROWSABLE, ni composant ni sélecteur explicites)
     * pour qu'une page ne puisse viser qu'une activité publique navigable.
     */
    private fun launchExternal(view: WebView, url: String) {
        val intent = try {
            if (url.startsWith("intent:", ignoreCase = true)) {
                Intent.parseUri(url, Intent.URI_INTENT_SCHEME)
            } else {
                Intent(Intent.ACTION_VIEW, Uri.parse(url))
            }
        } catch (e: URISyntaxException) {
            return
        }
        intent.addCategory(Intent.CATEGORY_BROWSABLE)
        intent.component = null
        intent.selector = null
        try {
            startActivity(intent)
        } catch (e: ActivityNotFoundException) {
            val fallback = intent.getStringExtra("browser_fallback_url")
            if (fallback != null && (fallback.startsWith("https://") || fallback.startsWith("http://"))) {
                view.loadUrl(fallback)
            } else {
                Toast.makeText(this, R.string.no_app, Toast.LENGTH_SHORT).show()
            }
        } catch (e: SecurityException) {
            Log.w(TAG, "Intent refusé", e)
        }
    }

    // ── Clients WebView ────────────────────────────────────────────────────

    private inner class TabClient(private val tab: BrowserTab) : WebViewClient() {

        override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
            val uri = request.url
            return when (uri.scheme?.lowercase()) {
                "http", "https" -> false
                "intent", "market", "tel", "mailto" -> {
                    launchExternal(view, uri.toString())
                    true
                }
                // Jamais depuis une page : système de fichiers, content providers, JS.
                "file", "content", "javascript" -> true
                // about:blank, blob:, data: dans les iframes — laissés au moteur.
                else -> request.isForMainFrame
            }
        }

        /** Thread réseau : aucun appel aux méthodes de WebView ici. */
        override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
            val uri = request.url
            if (NewTabPage.isAppAssets(uri)) {
                return assetLoader.shouldInterceptRequest(uri) ?: NewTabPage.notFound()
            }
            if (request.isForMainFrame) return null
            val scheme = uri.scheme?.lowercase()
            if (scheme != "http" && scheme != "https") return null
            val url = uri.toString()
            val headers = request.requestHeaders
            val accept = headers["Accept"] ?: headers["accept"] ?: ""
            return if (NyxCore.safeShouldBlock(url, tab.pageUrl, accept)) blockedResponse() else null
        }

        override fun onPageStarted(view: WebView, url: String?, favicon: Bitmap?) {
            tab.pageUrl = url ?: ""
            tab.title = ""
            if (tab === current) {
                refreshAddress()
                refreshNav()
            }
        }

        override fun onPageFinished(view: WebView, url: String?) {
            if (url != null) tab.pageUrl = url
            capture.onPageFinished(view, url)
            if (tab === current) {
                refreshAddress()
                refreshNav()
            }
        }

        override fun doUpdateVisitedHistory(view: WebView, url: String?, isReload: Boolean) {
            if (url != null) {
                tab.pageUrl = url
                VaultRepo.recordVisit(tab, url, isReload)
            }
            if (tab === current) {
                refreshAddress()
                refreshNav()
            }
        }

        /** Le renderer est mort : la WebView est inutilisable, on la remplace. */
        override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
            view.post { closeTab(tab) }
            return true
        }
    }

    private inner class TabChromeClient(private val tab: BrowserTab) : WebChromeClient() {
        override fun onProgressChanged(view: WebView, newProgress: Int) {
            tab.progress = newProgress
            if (tab === current) refreshProgress()
        }

        override fun onReceivedTitle(view: WebView, title: String?) {
            tab.title = title ?: ""
            VaultRepo.retitle(tab, tab.pageUrl, tab.title)
        }

        override fun onShowCustomView(view: View, callback: WebChromeClient.CustomViewCallback) {
            enterFullscreen(tab, view, callback)
        }

        override fun onHideCustomView() {
            if (customViewTab === tab) exitFullscreen(notifyPage = false)
        }

        override fun onPermissionRequest(request: PermissionRequest) {
            onPagePermissionRequest(tab, request)
        }

        override fun onPermissionRequestCanceled(request: PermissionRequest) {
            cancelPagePermission(tab, request)
        }

        override fun onGeolocationPermissionsShowPrompt(
            origin: String?,
            callback: GeolocationPermissions.Callback?,
        ) {
            if (origin == null || callback == null) return
            onGeolocationPrompt(tab, origin, callback)
        }

        override fun onGeolocationPermissionsHidePrompt() {
            onGeolocationHidden(tab)
        }

        override fun onShowFileChooser(
            webView: WebView,
            filePathCallback: ValueCallback<Array<Uri>>,
            fileChooserParams: WebChromeClient.FileChooserParams,
        ): Boolean = showFileChooser(tab, filePathCallback, fileChooserParams)
    }

    private companion object {
        const val TAG = "NyxBrowser"
        const val MENU_FIND = 1
        const val MENU_NEW_TAB = 2
        const val MENU_BOOKMARK = 3
        const val MENU_BOOKMARKS = 4
        const val MENU_HISTORY = 5
        const val MENU_PASSWORDS = 6
        const val MENU_SETTINGS = 7

        fun blockedResponse(): WebResourceResponse =
            WebResourceResponse(
                "text/plain", "utf-8", 403, "Blocked by NyxGuard",
                emptyMap(), ByteArrayInputStream(ByteArray(0)),
            )
    }
}
