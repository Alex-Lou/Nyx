package io.nyx.browser

import android.app.Activity
import android.text.TextUtils
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.ImageButton
import android.widget.LinearLayout
import android.widget.TextView
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat

/** Briques d'interface programmatiques, style Nyx sombre (écrans secondaires). */
object NyxUi {
    fun dp(activity: Activity, value: Int): Int = (value * activity.resources.displayMetrics.density).toInt()

    fun color(activity: Activity, id: Int): Int = ContextCompat.getColor(activity, id)

    /**
     * Racine plein écran : edge-to-edge (imposé par targetSdk 35 sur
     * Android 15), insets barres système + clavier appliqués en padding.
     */
    fun root(activity: Activity): LinearLayout {
        WindowCompat.setDecorFitsSystemWindows(activity.window, false)
        val root = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(color(activity, R.color.nyx_deep))
        }
        ViewCompat.setOnApplyWindowInsetsListener(root) { v, insets ->
            val bars = insets.getInsets(
                WindowInsetsCompat.Type.systemBars() or
                    WindowInsetsCompat.Type.displayCutout() or
                    WindowInsetsCompat.Type.ime(),
            )
            v.setPadding(bars.left, bars.top, bars.right, bars.bottom)
            WindowInsetsCompat.CONSUMED
        }
        return root
    }

    /** En-tête : retour + titre (+ action à droite, facultative). */
    fun header(activity: Activity, title: String, action: View? = null): LinearLayout {
        val bar = LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(activity, 4), dp(activity, 6), dp(activity, 12), dp(activity, 6))
        }
        bar.addView(
            ImageButton(activity).apply {
                setImageResource(R.drawable.ic_back)
                contentDescription = activity.getString(R.string.cd_back)
                setBackgroundResource(android.R.color.transparent)
                setOnClickListener { activity.finish() }
            },
            LinearLayout.LayoutParams(dp(activity, 46), dp(activity, 46)),
        )
        bar.addView(
            TextView(activity).apply {
                text = title
                textSize = 19f
                setTextColor(color(activity, R.color.nyx_text))
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        if (action != null) bar.addView(action)
        return bar
    }

    /** Titre de section. */
    fun section(activity: Activity, title: String): TextView = TextView(activity).apply {
        text = title
        textSize = 13f
        setAllCaps(true)
        setTextColor(color(activity, R.color.nyx_moon))
        setPadding(dp(activity, 16), dp(activity, 20), dp(activity, 16), dp(activity, 6))
    }

    /** Ligne deux niveaux (titre + sous-titre atténué), une ligne chacun. */
    fun twoLines(activity: Activity, title: String, subtitle: String): LinearLayout {
        val box = LinearLayout(activity).apply { orientation = LinearLayout.VERTICAL }
        box.addView(TextView(activity).apply {
            text = title
            textSize = 15f
            maxLines = 1
            ellipsize = TextUtils.TruncateAt.END
            setTextColor(color(activity, R.color.nyx_text))
        })
        if (subtitle.isNotEmpty()) {
            box.addView(TextView(activity).apply {
                text = subtitle
                textSize = 12f
                maxLines = 1
                ellipsize = TextUtils.TruncateAt.MIDDLE
                setTextColor(color(activity, R.color.nyx_dim))
            })
        }
        return box
    }

    /** Rangée horizontale de liste (fond surface, séparée de la suivante). */
    fun row(activity: Activity): LinearLayout = LinearLayout(activity).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        minimumHeight = dp(activity, 56)
        setPadding(dp(activity, 16), dp(activity, 8), dp(activity, 4), dp(activity, 8))
        setBackgroundColor(color(activity, R.color.nyx_surface))
        layoutParams = LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.WRAP_CONTENT,
        ).apply { bottomMargin = dp(activity, 1) }
    }

    fun iconButton(activity: Activity, icon: Int, description: String, onClick: () -> Unit): ImageButton =
        ImageButton(activity).apply {
            setImageResource(icon)
            contentDescription = description
            setBackgroundResource(android.R.color.transparent)
            setOnClickListener { onClick() }
            layoutParams = LinearLayout.LayoutParams(dp(activity, 44), dp(activity, 44))
        }

    /** Bouton texte discret (« Afficher », « Copier »…). */
    fun textButton(activity: Activity, label: String, onClick: () -> Unit): TextView = TextView(activity).apply {
        text = label
        textSize = 13f
        setTextColor(color(activity, R.color.nyx_aurora))
        gravity = Gravity.CENTER
        isClickable = true
        isFocusable = true
        setPadding(dp(activity, 10), dp(activity, 10), dp(activity, 10), dp(activity, 10))
        setOnClickListener { onClick() }
    }

    /** Texte d'état centré (liste vide, coffre verrouillé). */
    fun placeholder(activity: Activity, text: String): TextView = TextView(activity).apply {
        this.text = text
        textSize = 14f
        gravity = Gravity.CENTER
        setTextColor(color(activity, R.color.nyx_dim))
        setPadding(dp(activity, 24), dp(activity, 48), dp(activity, 24), dp(activity, 48))
    }
}
