import groovy.json.JsonOutput
import java.io.File
import java.util.Properties
import java.util.zip.ZipFile
import java.util.zip.ZipInputStream
import javax.inject.Inject
import org.gradle.api.artifacts.component.ModuleComponentIdentifier
import org.gradle.api.artifacts.component.ProjectComponentIdentifier
import org.gradle.api.artifacts.result.ResolvedArtifactResult
import org.gradle.process.ExecOperations

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

// The release key: the keystore ANDROID_KEYSTORE_PATH names, with ANDROID_KEYSTORE_PASSWORD,
// ANDROID_KEY_ALIAS and ANDROID_KEY_PASSWORD, as the release workflow sets them from its secrets.
// Without them a release build is signed with the standard debug key, for an APK that only goes
// to the builder's own devices; Android installs an update only with the installed one's key.
val releaseKeystore: String? = System.getenv("ANDROID_KEYSTORE_PATH")?.takeIf { it.isNotEmpty() }

android {
    compileSdk = 36
    // The NDK the Rust library is built with (release.yml's NDK_VERSION). Without
    // it AGP looks for its own default NDK to strip the library with, finds none,
    // and packs it whole: a release APK carried 17 MB of symbol tables.
    ndkVersion = "29.0.14206865"
    namespace = "com.faforever.rustclient"
    signingConfigs {
        create("release") {
            if (releaseKeystore != null) {
                storeFile = file(releaseKeystore)
                storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
            } else {
                logger.warn("ANDROID_KEYSTORE_PATH is not set: the release APK is signed with the debug key")
                storeFile = file("${System.getProperty("user.home")}/.android/debug.keystore")
                storePassword = "android"
                keyAlias = "androiddebugkey"
                keyPassword = "android"
            }
        }
    }
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.faforever.rustclient"
        // Android 8.0, which FAF's faf-uid for Android is built for. The Rust library is linked
        // for the same level, by tauri.android.conf.json's bundle.android.minSdkVersion.
        minSdk = 26
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            signingConfig = signingConfigs.getByName("release")
            isMinifyEnabled = true
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    // faf-uid (PrepareFafUid below) runs as a program, and Android runs an app's own programs
    // only from its native library folder, which it fills only when the APK's native libraries
    // are extracted on install. Packed the newer way they stay in the APK and the folder is empty.
    packaging {
        jniLibs.useLegacyPackaging = true
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
}

rust {
    rootDirRel = "../../../"
}

// The template keeps the native library's symbols for debug builds (`packaging`
// in the debug build type), but that setting is the whole app's, so release APKs
// carried 17 MB of symbol tables. Release variants strip them again, all but faf-uid: stripped,
// it would no longer be the file whose SHA-256 the client checks before it runs it.
androidComponents {
    onVariants(selector().withBuildType("release")) { variant ->
        variant.packaging.jniLibs.keepDebugSymbols.set(setOf("**/libfafuid.so"))
    }
}

// faf-uid, the program the lobby's sign-in runs, as <abi>/libfafuid.so with its licence text in
// assets/licenses: scripts/ensure-faf-uid-android.mjs fetches FAF's official build by the pin the
// client checks it against, and packs nothing while the pin is empty.
abstract class PrepareFafUid : DefaultTask() {
    @get:Input
    abstract val abis: ListProperty<String>

    @get:Internal
    abstract val repository: DirectoryProperty

    @get:OutputDirectory
    abstract val jniLibsDirectory: DirectoryProperty

    @get:OutputDirectory
    abstract val assetsDirectory: DirectoryProperty

    // What it packed, for the licence notice.
    @get:OutputFile
    abstract val shipped: RegularFileProperty

    @get:Inject
    abstract val execOperations: ExecOperations

    init {
        // It reads the pin and FAF_UID_ANDROID_TEST_BUILDS, out of Gradle's sight. A download is
        // kept in natives/android, so running again only checks the file's SHA-256.
        outputs.upToDateWhen { false }
    }

    @TaskAction
    fun prepare() {
        execOperations.exec {
            workingDir = repository.get().asFile
            commandLine(
                listOf(
                    "node", "scripts/ensure-faf-uid-android.mjs",
                    "--jni-libs", jniLibsDirectory.get().asFile.path,
                    "--assets", assetsDirectory.get().asFile.path,
                    "--shipped", shipped.get().asFile.path,
                ) + abis.get().flatMap { listOf("--abi", it) },
            )
        }
    }
}

// The licence and notice files in the jars and AARs on a variant's runtime classpath, for
// scripts/android-notices.mjs. AGP leaves META-INF/LICENSE and its kind out of the APK, so the
// notice is where their texts reach the app.
abstract class CollectLibraryNotices : DefaultTask() {
    // The downloaded libraries. Tauri's own are projects of this build: `projects`.
    @get:Internal
    abstract val libraries: SetProperty<ResolvedArtifactResult>

    // The same files, for Gradle to see when they change.
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.NAME_ONLY)
    abstract val libraryFiles: ConfigurableFileCollection

    // The Android library projects on the classpath, by path, with their directories.
    @get:Input
    abstract val projects: MapProperty<String, String>

    @get:OutputFile
    abstract val output: RegularFileProperty

    @TaskAction
    fun collect() {
        val found = libraries.get()
            .groupBy({ it.id.componentIdentifier }, { it.file })
            .map { (component, files) ->
                val id = component as? ModuleComponentIdentifier
                    ?: throw GradleException("$component is on the runtime classpath, and the licence notice has no name or version for it")
                mapOf(
                    "group" to id.group,
                    "name" to id.module,
                    "version" to id.version,
                    "texts" to files.flatMap { licenceEntries(it) },
                )
            }
            .sortedBy { "${it["group"]}:${it["name"]}" }
        // JsonOutput escapes every character past ASCII, so the em dash of a licence text never
        // trips scripts/check-architecture.mjs, which also reads the build's JSON files.
        output.get().asFile.writeText(JsonOutput.toJson(mapOf("libraries" to found, "projects" to projects.get())))
    }

    // A jar's licence files, at its top or under META-INF/, and those of the jars in an AAR
    // (classes.jar, libs/).
    private fun licenceEntries(file: File): List<Map<String, String>> {
        val texts = mutableListOf<Map<String, String>>()
        ZipFile(file).use { zip ->
            for (entry in zip.entries()) {
                if (entry.isDirectory) continue
                if (file.extension == "aar" && entry.name.endsWith(".jar")) {
                    ZipInputStream(zip.getInputStream(entry)).use { jar ->
                        while (true) {
                            val inner = jar.nextEntry ?: break
                            if (!inner.isDirectory && isLicenceFile(inner.name)) {
                                texts += text("${entry.name}!/${inner.name}", jar.readBytes())
                            }
                        }
                    }
                } else if (isLicenceFile(entry.name)) {
                    texts += text(entry.name, zip.getInputStream(entry).use { it.readBytes() })
                }
            }
        }
        return texts
    }

    // Named as scripts/android-notices.mjs names a licence file, and kept where a jar keeps
    // one: not a class or a service registration.
    private fun isLicenceFile(path: String): Boolean {
        val placed = '/' !in path || (path.startsWith("META-INF/") && !path.startsWith("META-INF/services/"))
        val name = path.substringAfterLast('/')
        return placed && !name.endsWith(".class") && LICENCE_NAME.containsMatchIn(name)
    }

    private fun text(entry: String, bytes: ByteArray) = mapOf("entry" to entry, "text" to String(bytes, Charsets.UTF_8))

    companion object {
        private val LICENCE_NAME = Regex("licen[cs]e|copying|copyright|notice", RegexOption.IGNORE_CASE)
    }
}

// assets/NOTICE.txt, the app's licence notice: scripts/android-notices.mjs writes it from what the
// build compiled, and fails the build when a package ships no licence text and has no override.
abstract class GenerateNotice : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val libraries: RegularFileProperty

    @get:Input
    abstract val rustTargets: ListProperty<String>

    // What PrepareFafUid packed.
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val fafUid: RegularFileProperty

    @get:Input
    abstract val release: Property<Boolean>

    @get:Internal
    abstract val repository: DirectoryProperty

    @get:OutputDirectory
    abstract val outputDirectory: DirectoryProperty

    @get:Inject
    abstract val execOperations: ExecOperations

    init {
        // Most of what the notice reads is out of Gradle's sight: cargo's resolution, the crates'
        // files and the page build's list. Writing it takes a few seconds, so it is written every
        // time rather than trusted from the build before.
        outputs.upToDateWhen { false }
    }

    @TaskAction
    fun generate() {
        val notice = outputDirectory.get().file("NOTICE.txt").asFile
        execOperations.exec {
            workingDir = repository.get().asFile
            commandLine(
                listOf(
                    "node", "scripts/android-notices.mjs", "--out", notice.path,
                    "--android-libraries", libraries.get().asFile.path, "--faf-uid", fafUid.get().asFile.path,
                ) +
                    rustTargets.get().flatMap { listOf("--rust-target", it) } +
                    (if (release.get()) listOf("--release") else emptyList()),
            )
        }
    }
}

// The Rust targets the native library is built for, as cargo names them: RustPlugin's targetList,
// which the Tauri CLI sets, with RustPlugin's default.
val rustTargetTriples = ((findProperty("targetList") as? String)?.split(',') ?: listOf("aarch64", "armv7", "i686", "x86_64"))
    .map { target ->
        mapOf(
            "aarch64" to "aarch64-linux-android",
            "armv7" to "armv7-linux-androideabi",
            "i686" to "i686-linux-android",
            "x86_64" to "x86_64-linux-android",
        )[target] ?: throw GradleException("No Rust target is known for the targetList entry $target")
    }
val projectDirectories = rootProject.allprojects.associate { it.path to it.projectDir.path }
// The ABIs the APK is built for: RustPlugin's abiList, which the Tauri CLI sets, with its default.
val abiList = (findProperty("abiList") as? String)?.split(',') ?: listOf("arm64-v8a", "armeabi-v7a", "x86", "x86_64")

// Every APK carries assets/NOTICE.txt, generated, never kept by hand, and faf-uid when it is pinned.
androidComponents {
    onVariants { variant ->
        val name = variant.name.replaceFirstChar { it.uppercase() }
        val prepareFafUid = tasks.register<PrepareFafUid>("prepare${name}FafUid") {
            abis.set(abiList)
            repository.set(rootProject.layout.projectDirectory.dir("../../.."))
            shipped.set(layout.buildDirectory.file("faf-uid/${variant.name}/shipped.json"))
        }
        variant.sources.jniLibs?.addGeneratedSourceDirectory(prepareFafUid, PrepareFafUid::jniLibsDirectory)
        variant.sources.assets?.addGeneratedSourceDirectory(prepareFafUid, PrepareFafUid::assetsDirectory)
        val classpath = variant.runtimeConfiguration.incoming
        val downloaded = classpath.artifactView { componentFilter { it !is ProjectComponentIdentifier } }.artifacts
        // The app itself is the root of the graph, not one of its libraries.
        val libraryProjects = provider {
            classpath.resolutionResult.allComponents
                .mapNotNull { it.id as? ProjectComponentIdentifier }
                .filter { it.projectPath != project.path }
                .associate { it.projectPath to projectDirectories.getValue(it.projectPath) }
        }
        val collect = tasks.register<CollectLibraryNotices>("collect${name}LibraryNotices") {
            libraries.set(downloaded.resolvedArtifacts)
            libraryFiles.from(downloaded.artifactFiles)
            projects.set(libraryProjects)
            output.set(layout.buildDirectory.file("notices/${variant.name}/libraries.json"))
        }
        val notice = tasks.register<GenerateNotice>("generate${name}Notice") {
            libraries.set(collect.flatMap { it.output })
            rustTargets.set(rustTargetTriples)
            fafUid.set(prepareFafUid.flatMap { it.shipped })
            release.set(variant.buildType == "release")
            repository.set(rootProject.layout.projectDirectory.dir("../../.."))
            // After cargo has built the library and fetched its crates, so that cargo metadata
            // reads the lockfile that build used and downloads nothing itself.
            mustRunAfter(tasks.matching { it.name.startsWith("rustBuild") })
        }
        variant.sources.assets?.addGeneratedSourceDirectory(notice, GenerateNotice::outputDirectory)
    }
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")