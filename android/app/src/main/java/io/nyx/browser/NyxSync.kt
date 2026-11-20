package io.nyx.browser

/**
 * Pont JNI « Appareils liés » (`crates/nyx-ffi`, modules `sync_api` et
 * `jni_exports::sync`). Toute la logique vit en Rust (`nyx-sync`) ; ces
 * déclarations ne font que marshaller.
 *
 * Conventions : `String` vide = succès, sinon message affichable ; états et
 * listes en JSON. Les méthodes marquées « bloquant » font du réseau : ne
 * jamais les appeler sur le thread UI (passer par [SyncRepo]).
 */
object NyxSync {
    /** Même bibliothèque que [NyxCore] (chargée par son initialiseur). */
    val loaded: Boolean get() = NyxCore.loaded

    /** Bloquant : rejoint le PC de l'offre `nyxpair:v1:…` ; rend le code à comparer. */
    @JvmStatic external fun pairBegin(offer: String, deviceName: String): String?

    /** Bloquant : réponse de l'utilisateur, rend le résultat final. */
    @JvmStatic external fun pairConfirm(yes: Boolean): String?

    @JvmStatic external fun pairCancel()

    /** Bloquant (~3 s de découverte mDNS + sessions). */
    @JvmStatic external fun syncNow(): String?

    @JvmStatic external fun devicesJson(): String?
    @JvmStatic external fun setDeviceName(name: String): String?
    @JvmStatic external fun revoke(deviceId: String): String?
    @JvmStatic external fun lastSync(): String?
}
