package io.nyx.browser

/**
 * Pont JNI vers le coffre SQLCipher (`crates/nyx-ffi`, module `vault_api`).
 * Toute la logique vit en Rust ; ces déclarations ne font que marshaller.
 *
 * Conventions : une `String` vide = succès, sinon message d'erreur
 * affichable (jamais de clé dedans) ; listes en JSON ; identifiant `-1` =
 * échec. Les clés d'appareil passent en [ByteArray] : Rust efface sa copie,
 * l'appelant Kotlin efface la sienne (`fill(0)`) juste après l'appel.
 *
 * Ne jamais appeler ces méthodes si [NyxCore.loaded] est faux : passer par
 * [VaultRepo], qui dégrade proprement (coffre indisponible).
 */
object NyxVault {
    /** Même bibliothèque que [NyxCore] (chargée par son initialiseur). */
    val loaded: Boolean get() = NyxCore.loaded

    // Démarrage, ouverture, création
    @JvmStatic external fun generateDeviceKey(): ByteArray?
    @JvmStatic external fun startup(dbPath: String, keyFileExists: Boolean): String?
    @JvmStatic external fun checkNewPassphrase(passphrase: String, confirm: String): String?
    @JvmStatic external fun openWithDeviceKey(dbPath: String, key: ByteArray): String?
    @JvmStatic external fun openWithPassphrase(dbPath: String, passphrase: String): String?
    @JvmStatic external fun createWithDeviceKey(dbPath: String, key: ByteArray): String?
    @JvmStatic external fun createWithPassphrase(dbPath: String, passphrase: String, confirm: String): String?
    @JvmStatic external fun close()
    @JvmStatic external fun isOpen(): Boolean
    @JvmStatic external fun hasMasterPassword(): Boolean

    // Mot de passe principal
    @JvmStatic external fun verifyMaster(passphrase: String): String?
    @JvmStatic external fun setMaster(newPassphrase: String, confirm: String, currentKey: ByteArray): String?
    @JvmStatic external fun changeMaster(old: String, newPassphrase: String, confirm: String): String?
    @JvmStatic external fun removeMaster(current: String, newKey: ByteArray): String?

    // Mots de passe
    @JvmStatic external fun passwordsJson(): String?
    @JvmStatic external fun passwordsForHost(host: String): String?
    @JvmStatic external fun passwordDomainsJson(): String?
    @JvmStatic external fun passwordReveal(domain: String, id: Long): String?
    @JvmStatic external fun passwordAdd(domain: String, username: String, password: String): Long
    @JvmStatic external fun passwordUpdate(id: Long, password: String): Boolean
    @JvmStatic external fun passwordDelete(id: Long): Boolean

    // Capture et pré-remplissage
    @JvmStatic external fun captureOffer(sourceOrigin: String, topUrl: String, message: String, isPrivate: Boolean): String?
    @JvmStatic external fun captureSave(host: String, message: String): Boolean
    @JvmStatic external fun autofillScript(pageUrl: String, isPrivate: Boolean): String?

    // Favoris
    @JvmStatic external fun bookmarksJson(): String?
    @JvmStatic external fun bookmarkFind(url: String): Long
    @JvmStatic external fun bookmarkAdd(url: String, title: String): Long
    @JvmStatic external fun bookmarkDelete(id: Long): Boolean

    // Historique
    @JvmStatic external fun historyPush(url: String, title: String): Boolean
    @JvmStatic external fun historyRetitle(url: String, title: String): Boolean
    @JvmStatic external fun historyRecentJson(limit: Int): String?
    @JvmStatic external fun historySearchJson(query: String): String?
    @JvmStatic external fun historyClear(): Boolean

    // Réglages
    @JvmStatic external fun settingGet(key: String): String?
    @JvmStatic external fun settingSet(key: String, value: String): Boolean
    @JvmStatic external fun settingsJson(): String?
    @JvmStatic external fun settingsUpdate(engine: String, home: String, adblock: Boolean, savePasswords: Boolean): String?
    @JvmStatic external fun homeUrl(): String?
}
