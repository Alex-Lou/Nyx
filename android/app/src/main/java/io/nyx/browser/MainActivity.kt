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
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.view.animation.DecelerateInterpolator
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.webkit.CookieManager
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
import androidx.webkit.WebSettingsCompat
import androidx.webkit.WebViewAssetLoader
import androidx.webkit.WebViewFeature
import com.google.android.material.bottomsheet.BottomSheetDialog
import com.google.android.material.button.MaterialButton
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
    private lateinit var progress: ProgressBar
    private lateinit var assetLoader: WebViewAssetLoader

    /** Suite d'une demande de permissions Android en cours (une seule à la fois). */
    private var pendingPermissions: (() -> Unit)? = null
    private val permissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
            // Résultat relu via checkSelfPermission : seule source de vérité.
            val next = pendingPermissions
            pendingPermissions = null
            next?.invoke()
        }

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
        progress = findViewById(R.id.progress)

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
        bindChrome()

        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() = onBack()
        })

        openTab(viewIntentUrl(intent))
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
        webView.setDownloadListener { url, userAgent, contentDisposition, mimeType, _ ->
            startDownload(url, userAgent, contentDisposition, mimeType)
        }
        tabs.add(tab)
        switchTo(tab)
        load(tab, url ?: NewTabPage.URL)
    }

    private fun switchTo(tab: BrowserTab) {
        val previous = current
        if (customViewTab != null && customViewTab !== tab) exitFullscreen(notifyPage = true)
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
        if (current === tab) current = null
        releaseTab(tab)
        destroyWebView(tab.webView)
        when {
            tabs.isEmpty() -> openTab(null)
            current == null -> switchTo(tabs[minOf(index, tabs.size - 1)])
            else -> refreshTabCount()
        }
    }

    /** Rend la main à la page : aucun callback WebView ne survit à l'onglet. */
    private fun releaseTab(tab: BrowserTab) {
        if (fileCallbackTab === tab) cancelFileChooser()
        if (customViewTab === tab) exitFullscreen(notifyPage = true)
    }

    /** Aucune fuite : détachée du parent, chargement arrêté, puis détruite. */
    private fun destroyWebView(webView: WebView) {
        (webView.parent as? ViewGroup)?.removeView(webView)
        webView.stopLoading()
        webView.setDownloadListener(null)
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
    }

    override fun onPause() {
        current?.webView?.onPause()
        CookieManager.getInstance().flush()
        super.onPause()
    }

    override fun onDestroy() {
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
            if (tab === current) {
                refreshAddress()
                refreshNav()
            }
        }

        override fun doUpdateVisitedHistory(view: WebView, url: String?, isReload: Boolean) {
            if (url != null) tab.pageUrl = url
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
        }

        override fun onShowCustomView(view: View, callback: WebChromeClient.CustomViewCallback) {
            enterFullscreen(tab, view, callback)
        }

        override fun onHideCustomView() {
            if (customViewTab === tab) exitFullscreen(notifyPage = false)
        }

        override fun onShowFileChooser(
            webView: WebView,
            filePathCallback: ValueCallback<Array<Uri>>,
            fileChooserParams: WebChromeClient.FileChooserParams,
        ): Boolean = showFileChooser(tab, filePathCallback, fileChooserParams)
    }

    private companion object {
        const val TAG = "NyxBrowser"

        fun blockedResponse(): WebResourceResponse =
            WebResourceResponse(
                "text/plain", "utf-8", 403, "Blocked by NyxGuard",
                emptyMap(), ByteArrayInputStream(ByteArray(0)),
            )
    }
}
