import java.io.File
import org.apache.tools.ant.taskdefs.condition.Os
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.logging.LogLevel
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.TaskAction

// Code for the body of the WebView client wry generates (generated/RustWebViewClient.kt), which
// wry's build script takes from this variable when the Rust build writes that file: the one way
// in short of editing a generated file. It answers the page's renderer process ending, which wry
// leaves unanswered and Android then ends the whole client for; MainActivity.onRendererGone says
// why and what it does.
private const val WEBVIEW_CLIENT_EXTENSION_VARIABLE = "WRY_RUSTWEBVIEWCLIENT_CLASS_EXTENSION"
private val webViewClientExtension = """
    override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean =
        (view.context as? MainActivity)?.onRendererGone(view, detail) ?: false
""".trimIndent()

open class BuildTask : DefaultTask() {
    @Input
    var rootDirRel: String? = null
    @Input
    var target: String? = null
    @Input
    var release: Boolean? = null

    @TaskAction
    fun assemble() {
        val executable = """pnpm""";
        try {
            runTauriCli(executable)
        } catch (e: Exception) {
            if (Os.isFamily(Os.FAMILY_WINDOWS)) {
                // Try different Windows-specific extensions
                val fallbacks = listOf(
                    "$executable.exe",
                    "$executable.cmd",
                    "$executable.bat",
                )
                
                var lastException: Exception = e
                for (fallback in fallbacks) {
                    try {
                        runTauriCli(fallback)
                        return
                    } catch (fallbackException: Exception) {
                        lastException = fallbackException
                    }
                }
                throw lastException
            } else {
                throw e;
            }
        }
    }

    fun runTauriCli(executable: String) {
        val rootDirRel = rootDirRel ?: throw GradleException("rootDirRel cannot be null")
        val target = target ?: throw GradleException("target cannot be null")
        val release = release ?: throw GradleException("release cannot be null")
        // `pnpm exec`, not the package script `pnpm tauri`: that one first downloads the desktop
        // helpers (faf-uid, the ICE adapters, a Java runtime), which an Android build has no use for.
        val args = listOf("exec", "tauri", "android", "android-studio-script");

        project.exec {
            workingDir(File(project.projectDir, rootDirRel))
            executable(executable)
            environment(WEBVIEW_CLIENT_EXTENSION_VARIABLE, webViewClientExtension)
            args(args)
            if (project.logger.isEnabled(LogLevel.DEBUG)) {
                args("-vv")
            } else if (project.logger.isEnabled(LogLevel.INFO)) {
                args("-v")
            }
            if (release) {
                args("--release")
            }
            args(listOf("--target", target))
        }.assertNormalExitValue()
    }
}