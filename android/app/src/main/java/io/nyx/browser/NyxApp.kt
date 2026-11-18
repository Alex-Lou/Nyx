package io.nyx.browser

import android.app.Application
import android.util.Log

/**
 * Charge EasyList + EasyPrivacy (assets `filterlists/` en `.txt.gz`, partagés avec
 * le desktop) dans le moteur Rust, hors du thread UI : le parse prend environ
 * une seconde. En attendant, seules les règles intégrées de NyxGuard filtrent.
 */
class NyxApp : Application() {

    override fun onCreate() {
        super.onCreate()
        val worker = Thread({ loadFilterLists() }, "nyx-filters")
        worker.priority = Thread.NORM_PRIORITY - 1
        worker.start()
    }

    private fun loadFilterLists() {
        try {
            if (!NyxCore.loaded) return
            val lists = FILTER_ASSETS.map { path ->
                assets.open(path).use { it.readBytes() }
            }.toTypedArray()
            val ok = NyxCore.initFilters(lists)
            Log.i(TAG, "Listes de filtrage chargées : $ok")
        } catch (t: Throwable) {
            Log.e(TAG, "Échec du chargement des listes de filtrage", t)
        }
    }

    private companion object {
        const val TAG = "NyxApp"
        val FILTER_ASSETS = listOf(
            "filterlists/easylist.txt.gz",
            "filterlists/easyprivacy.txt.gz",
        )
    }
}
