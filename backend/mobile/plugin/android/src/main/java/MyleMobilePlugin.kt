package uk.thomast.myle.mobile

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.view.ViewGroup
import android.webkit.WebView
import androidx.browser.customtabs.CustomTabColorSchemeParams
import androidx.browser.customtabs.CustomTabsIntent
import androidx.core.content.FileProvider
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File

/** The app's background (frontend/styles/tokens.css, --bg-0): behind the
 *  status and navigation bars, and the sign-in tab's toolbar. */
private val BACKGROUND = Color.rgb(0x0a, 0x0c, 0x12)

@InvokeArg
class SignInArgs {
    var url: String = ""
    var callbackScheme: String = ""
}

@InvokeArg
class InstallArgs {
    var path: String = ""
}

@TauriPlugin
class MyleMobilePlugin(private val activity: Activity) : Plugin(activity) {
    override fun load(webView: WebView) {
        super.load(webView)
        fitScreen(webView)
    }

    /**
     * Android draws apps edge to edge (always from Android 15): the page is
     * kept clear of the status bar, the navigation bar, the camera cutout and
     * the keyboard, with the app's background behind the bars and light
     * icons on it.
     */
    private fun fitScreen(webView: WebView) {
        activity.runOnUiThread {
            val window = activity.window
            window.decorView.setBackgroundColor(BACKGROUND)
            WindowCompat.getInsetsController(window, window.decorView).apply {
                isAppearanceLightStatusBars = false
                isAppearanceLightNavigationBars = false
            }
            ViewCompat.setOnApplyWindowInsetsListener(webView) { view, insets ->
                val bars = insets.getInsets(
                    WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
                )
                val keyboard = insets.getInsets(WindowInsetsCompat.Type.ime())
                (view.layoutParams as? ViewGroup.MarginLayoutParams)?.let { params ->
                    val bottom = maxOf(bars.bottom, keyboard.bottom)
                    if (params.leftMargin != bars.left || params.topMargin != bars.top ||
                        params.rightMargin != bars.right || params.bottomMargin != bottom
                    ) {
                        params.setMargins(bars.left, bars.top, bars.right, bottom)
                        view.layoutParams = params
                    }
                }
                WindowInsetsCompat.CONSUMED
            }
            ViewCompat.requestApplyInsets(webView)
        }
    }

    /**
     * The sign-in page in a Custom Tab: the browser's page over the app, with
     * the browser's own sign-ins. It comes back to the app as a link
     * (uk.thomast.myle.passwords://auth-callback), which the app takes; this
     * answers at once.
     */
    @Command
    fun signIn(invoke: Invoke) {
        val args = invoke.parseArgs(SignInArgs::class.java)
        activity.runOnUiThread {
            try {
                val colors = CustomTabColorSchemeParams.Builder()
                    .setToolbarColor(BACKGROUND)
                    .setNavigationBarColor(BACKGROUND)
                    .build()
                val tab = CustomTabsIntent.Builder()
                    .setDefaultColorSchemeParams(colors)
                    .setColorScheme(CustomTabsIntent.COLOR_SCHEME_DARK)
                    .setShowTitle(true)
                    .setShareState(CustomTabsIntent.SHARE_STATE_OFF)
                    .setUrlBarHidingEnabled(false)
                    .build()
                tab.launchUrl(activity, Uri.parse(args.url))
                invoke.resolve(JSObject())
            } catch (e: ActivityNotFoundException) {
                invoke.reject("No web browser is installed for signing in.")
            } catch (e: Exception) {
                invoke.reject(e.message ?: "The sign-in page could not open.")
            }
        }
    }

    /**
     * Shows Android's installer over the downloaded APK. The first time,
     * Android needs the user to allow this app to install updates: the
     * setting opens instead, and the page offers Install again.
     */
    @Command
    fun install(invoke: Invoke) {
        val args = invoke.parseArgs(InstallArgs::class.java)
        val file = File(args.path)
        if (!file.isFile) {
            invoke.reject("The update is not downloaded any more. Download it again.")
            return
        }
        activity.runOnUiThread {
            try {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
                    !activity.packageManager.canRequestPackageInstalls()
                ) {
                    activity.startActivity(
                        Intent(
                            Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                            Uri.parse("package:${activity.packageName}")
                        )
                    )
                    invoke.reject("allow-installs")
                    return@runOnUiThread
                }
                // The template's FileProvider (AndroidManifest.xml, res/xml/file_paths.xml):
                // the app's cache folder, where the update was downloaded.
                val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", file)
                val intent = Intent(Intent.ACTION_VIEW).apply {
                    setDataAndType(uri, "application/vnd.android.package-archive")
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
                activity.startActivity(intent)
                invoke.resolve()
            } catch (e: Exception) {
                invoke.reject(e.message ?: "Android's installer could not open.")
            }
        }
    }
}
