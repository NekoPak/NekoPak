"""Apply the checked-in Android host to Tauri's disposable generated project."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
VERSIONS = {
    "compile_sdk": 36,
    "target_sdk": 36,
    "min_sdk": 24,
    "build_tools": "36.0.0",
    "ndk": "27.2.12479018",
    "agp": "8.11.0",
    "gradle": "8.14.3",
    "kotlin": "2.2.10",
    "gradle_sha256": "bd71102213493060956ec229d946beee57158dbd89d0e62b91bca0fa2c5f3531",
}
ANDROID_NS = "http://schemas.android.com/apk/res/android"
ET.register_namespace("android", ANDROID_NS)


def replace_once(text: str, pattern: str, value: str, label: str) -> str:
    text, count = re.subn(pattern, lambda _: value, text, count=1, flags=re.MULTILINE)
    if count != 1:
        raise ValueError(f"Unexpected Tauri Android template: {label}")
    return text


def marked(text: str, name: str, content: str) -> str:
    begin, end = f"// NEKOPAK {name} BEGIN", f"// NEKOPAK {name} END"
    text = re.sub(re.escape(begin) + r".*?" + re.escape(end) + r"\n?", "", text, flags=re.S)
    return text.rstrip() + f"\n\n{begin}\n{content.rstrip()}\n{end}\n"


def copy_kotlin(source: Path, destination: Path) -> list[str]:
    copied = []
    for file in sorted(source.rglob("*.kt")):
        content = file.read_text(encoding="utf-8")
        match = re.search(r"^package\s+([A-Za-z_][A-Za-z0-9_.]*)\s*$", content, re.M)
        if not match or ".." in match[1]:
            raise ValueError(f"Missing or invalid Kotlin package: {file.name}")
        target = destination.joinpath(*match[1].split("."), file.name)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8", newline="\n")
        copied.append(str(file.relative_to(source)))
    return copied


def sync_android_icons(root: Path, resources: Path) -> dict:
    source = root / "src-tauri/icons/android"
    intended = root.resolve() / "src-tauri/gen/android/app/src/main/res"
    if resources.resolve() != intended or resources.is_symlink():
        raise ValueError("Refusing to modify launcher resources outside the generated Android project.")
    required = [source / bucket / name for bucket in ["mipmap-anydpi-v26", "mipmap-anydpi-v33"]
                for name in ["ic_launcher.xml", "ic_launcher_round.xml"]]
    if not all(file.is_file() for file in required):
        raise ValueError("Committed Android normal/round adaptive icons for API 26 and 33 are missing.")
    files = sorted(file for file in source.rglob("*") if file.is_file())
    if any(file.is_symlink() or not file.resolve().is_relative_to(source.resolve()) for file in files):
        raise ValueError("Android icon sources must be ordinary repository files.")
    expected = {file.relative_to(source).as_posix() for file in files}
    removed = []
    # Tauri init installs its own launcher assets. Delete only stale, explicitly
    # named launcher files; unrelated application resources are never removed.
    for file in resources.rglob("ic_launcher*"):
        if file.is_file() and file.relative_to(resources).as_posix() not in expected:
            if file.is_symlink() or not file.resolve().is_relative_to(intended):
                raise ValueError("Refusing to delete a launcher symlink or escaped generated path.")
            removed.append(file.relative_to(resources).as_posix())
            file.unlink()
    for file in files:
        target = resources / file.relative_to(source)
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.is_symlink() or not target.resolve().is_relative_to(intended):
            raise ValueError("Refusing to replace an escaped Android resource path.")
        shutil.copyfile(file, target)
    return {"source": "src-tauri/icons/android", "files": {
        file.relative_to(source).as_posix(): hashlib.sha256(file.read_bytes()).hexdigest() for file in files
    }, "removed_template_resources": sorted(removed)}


def capture_generated_icons(root: Path = ROOT) -> None:
    """Persist Tauri icon output even when its Android special case targets gen/."""
    destination = root / "src-tauri/icons/android"
    generated = root / "src-tauri/gen/android/app/src/main/res"
    source = generated if (generated / "mipmap-mdpi/ic_launcher_monochrome.png").is_file() else destination
    if not (source / "mipmap-mdpi/ic_launcher_monochrome.png").is_file():
        raise ValueError("Run Tauri icon with public/brand/app-icons.json before capturing Android resources.")
    names = ["ic_launcher.png", "ic_launcher_round.png", "ic_launcher_foreground.png", "ic_launcher_background.png", "ic_launcher_monochrome.png"]
    for density in ["mdpi", "hdpi", "xhdpi", "xxhdpi", "xxxhdpi"]:
        for name in names:
            file = source / f"mipmap-{density}" / name
            if not file.is_file():
                raise ValueError(f"Incomplete generated Android icon set: {density}/{name}")
            target = destination / file.relative_to(source)
            target.parent.mkdir(parents=True, exist_ok=True)
            # Preserve the established legacy tile. Tauri derives legacy icons
            # by enlarging android_fg, which is composed for the adaptive safe
            # zone here; using that crop would change the existing brand scale.
            if name in ("ic_launcher.png", "ic_launcher_round.png") and target.is_file():
                continue
            if file.resolve() != target.resolve():
                shutil.copyfile(file, target)
    for api in (26, 33):
        icon = ET.Element("adaptive-icon")
        for role in ["background", "foreground"] + (["monochrome"] if api == 33 else []):
            ET.SubElement(icon, role, {f"{{{ANDROID_NS}}}drawable": f"@mipmap/ic_launcher_{role}"})
        tree = ET.ElementTree(icon)
        ET.indent(tree, space="  ")
        directory = destination / f"mipmap-anydpi-v{api}"
        directory.mkdir(parents=True, exist_ok=True)
        for name in ["ic_launcher.xml", "ic_launcher_round.xml"]:
            tree.write(directory / name, encoding="utf-8", xml_declaration=True)
    # Older single-input generation used a color instead of the branded PNG.
    obsolete = destination / "values/ic_launcher_background.xml"
    if obsolete.is_file():
        if obsolete.is_symlink() or not obsolete.resolve().is_relative_to(destination.resolve()):
            raise ValueError("Refusing to remove an unexpected old launcher color resource.")
        obsolete.unlink()
    print("Persisted Android icon manifest output and API26/API33 normal, round and themed descriptors.")


def generate_icons(root: Path = ROOT) -> None:
    # Depending on whether android init ran, Tauri writes into either gen/res or
    # the requested output directory. Snapshot existing legacy tiles before the
    # command so both paths preserve the established non-adaptive mark.
    icons = root / "src-tauri/icons/android"
    legacy = {file: file.read_bytes() for file in icons.glob("mipmap-*/*.png")
              if file.name in ("ic_launcher.png", "ic_launcher_round.png")}
    subprocess.run(["node", "node_modules/@tauri-apps/cli/tauri.js", "icon",
                    "public/brand/app-icons.json", "--output", "src-tauri/icons"], cwd=root, check=True)
    for file, data in legacy.items():
        file.write_bytes(data)
    capture_generated_icons(root)


def prepare(root: Path = ROOT) -> dict:
    project = root / "src-tauri/gen/android"
    if not (project / "app/build.gradle.kts").is_file():
        raise ValueError("Run npm run tauri -- android init --ci first.")
    app_file = project / "app/build.gradle.kts"
    app = app_file.read_text(encoding="utf-8")
    app = replace_once(app, r"compileSdk\s*=\s*\d+", "compileSdk = 36", "compile SDK")
    app = replace_once(app, r"targetSdk\s*=\s*\d+", "targetSdk = 36", "target SDK")
    app = replace_once(app, r"minSdk\s*=\s*\d+", "minSdk = 24", "minimum SDK")
    app = re.sub(r"optimization\s*\{\s*enable\s*=\s*true\s*\}", "isMinifyEnabled = true", app)
    # AGP 9-only flags must not be carried into the pinned AGP 8 build.
    props = project / "gradle.properties"
    props_text = props.read_text(encoding="utf-8")
    props_text = re.sub(r"^android\.(builtInKotlin|newDsl)=.*\n?", "", props_text, flags=re.M)
    props_text = re.sub(r"^org\.gradle\.configuration-cache=.*", "org.gradle.configuration-cache=false", props_text, flags=re.M)
    props_text = re.sub(r"^android\.useAndroidX=.*\n?", "", props_text, flags=re.M)
    props_text = props_text.rstrip() + "\nandroid.useAndroidX=true\n"
    # Cargo registry and workspace may be on different Windows drives. Kotlin's
    # relocatable incremental cache cannot relativize those paths reliably.
    props_text = re.sub(r"^kotlin\.incremental=.*\n?", "", props_text, flags=re.M)
    props_text = props_text.rstrip() + "\nkotlin.incremental=false\n"
    props.write_text(props_text, encoding="utf-8", newline="\n")

    app = marked(app, "ANDROID HOST", '''android {
    buildToolsVersion = "36.0.0"
    ndkVersion = "27.2.12479018"
    defaultConfig {
        testInstrumentationRunner = "android.test.InstrumentationTestRunner"
    }
    useLibrary("android.test.runner")
    useLibrary("android.test.base")
    useLibrary("android.test.mock")
    signingConfigs {
        create("nekopakRelease") {
            storeFile = System.getenv("ANDROID_KEYSTORE_PATH")?.let { file(it) }
            storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
            keyAlias = System.getenv("ANDROID_KEY_ALIAS")
            keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
        }
    }
    buildTypes.getByName("release") {
        signingConfig = signingConfigs.getByName("nekopakRelease")
        // The upstream template supplies project rules only. Android's default
        // rules preserve enum values()/valueOf(), runtime annotations and JNI
        // semantics required by Jackson inside Tauri's PluginManager.
        proguardFile(getDefaultProguardFile("proguard-android-optimize.txt"))
        // Cargo's release profile already strips symbols. Preserve those exact
        // bytes so packaging can reject a stale or substituted native library.
        packaging.jniLibs.keepDebugSymbols.add("**/libnekopak_app.so")
    }
}

// Debug builds never need or consume release secrets. A release request fails
// before compiling if any signing input is absent; no debug-key fallback.
val releaseRequested = gradle.startParameter.taskNames.any {
    it.contains("release", ignoreCase = true)
}
if (releaseRequested) {
    listOf("ANDROID_KEYSTORE_PATH", "ANDROID_KEYSTORE_PASSWORD", "ANDROID_KEY_ALIAS", "ANDROID_KEY_PASSWORD").forEach {
        check(!System.getenv(it).isNullOrBlank()) { "Missing Android release signing input: $it" }
    }
    check(file(System.getenv("ANDROID_KEYSTORE_PATH")).isFile) { "Android release keystore file is missing" }
}

tasks.register("nekopakLicenseInventory") {
    doLast {
        val selectedFlavor = listOf("x86_64", "arm64", "universal").firstOrNull { flavor ->
            gradle.startParameter.taskNames.any { it.contains(flavor, ignoreCase = true) }
        } ?: "universal"
        val configurationName = providers.gradleProperty("nekopakLicenseConfiguration").getOrElse(
            selectedFlavor + if (releaseRequested) "ReleaseRuntimeClasspath" else "DebugRuntimeClasspath"
        )
        // Project variants expose many Android-specific artifact types. Their
        // licenses are in Cargo; select only external JVM modules here.
        val artifacts = configurations.getByName(configurationName).incoming.artifactView {
            componentFilter { it is org.gradle.api.artifacts.component.ModuleComponentIdentifier }
        }.artifacts.artifacts
        // A fresh runner may resolve Gradle module metadata without having the
        // primary Maven POM in its cache. Fetch it explicitly for license audit.
        val pomQuery = dependencies.createArtifactResolutionQuery()
            .forComponents(artifacts.map { it.id.componentIdentifier })
            .withArtifacts(org.gradle.maven.MavenModule::class.java, org.gradle.maven.MavenPomArtifact::class.java)
            .execute()
        check(pomQuery.components.none { it is org.gradle.api.artifacts.result.UnresolvedComponentResult }) {
            "Android Maven license metadata could not be resolved"
        }
        pomQuery.resolvedComponents.forEach { component ->
            val poms = component.getArtifacts(org.gradle.maven.MavenPomArtifact::class.java)
            check(poms.isNotEmpty() && poms.all { it is org.gradle.api.artifacts.result.ResolvedArtifactResult }) {
                "Android Maven license POM is missing: ${component.id}"
            }
        }
        val records = artifacts.sortedBy { it.id.componentIdentifier.toString() }.map {
            val id = it.id.componentIdentifier as org.gradle.api.artifacts.component.ModuleComponentIdentifier
            mapOf("group" to id.group, "name" to id.module,
                  "version" to id.version, "file" to it.file.absolutePath, "project" to false)
        }
        val output = layout.buildDirectory.file("reports/nekopak/android-dependencies.json").get().asFile
        output.parentFile.mkdirs()
        output.writeText(groovy.json.JsonOutput.prettyPrint(groovy.json.JsonOutput.toJson(records)))
    }
}

val nekopakLicenses = tasks.register<Exec>("nekopakPrepareLicenses") {
    dependsOn("nekopakLicenseInventory")
    workingDir(file("../../../.."))
    commandLine(System.getenv("NEKOPAK_PYTHON") ?: "python", "scripts/package_android.py", "licenses",
        "--gradle-inventory", layout.buildDirectory.file("reports/nekopak/android-dependencies.json").get().asFile.absolutePath,
        "--assets-dir", file("src/main/assets/nekopak-licenses").absolutePath)
}
tasks.named("preBuild").configure { dependsOn(nekopakLicenses) }''')
    app_file.write_text(app, encoding="utf-8", newline="\n")
    for file in [project / "build.gradle.kts", project / "buildSrc/build.gradle.kts"]:
        text = file.read_text(encoding="utf-8")
        text = replace_once(text, r"com\.android\.tools\.build:gradle:[0-9.]+", "com.android.tools.build:gradle:8.11.0", "AGP version")
        text = re.sub(r"org\.jetbrains\.kotlin:kotlin-gradle-plugin:[0-9.]+", "org.jetbrains.kotlin:kotlin-gradle-plugin:2.2.10", text)
        file.write_text(text, encoding="utf-8", newline="\n")
    wrapper = project / "gradle/wrapper/gradle-wrapper.properties"
    text = wrapper.read_text(encoding="utf-8")
    text = replace_once(text, r"^distributionUrl=.*", r"distributionUrl=https\://services.gradle.org/distributions/gradle-8.14.3-bin.zip", "Gradle distribution")
    text = re.sub(r"^distributionSha256Sum=.*\n?", "", text, flags=re.M)
    wrapper.write_text(text.rstrip() + f"\ndistributionSha256Sum={VERSIONS['gradle_sha256']}\n", encoding="utf-8", newline="\n")

    manifest_file = project / "app/src/main/AndroidManifest.xml"
    tree = ET.parse(manifest_file)
    application = tree.getroot().find("application")
    if application is None:
        raise ValueError("Android manifest has no application.")
    application.set(f"{{{ANDROID_NS}}}icon", "@mipmap/ic_launcher")
    application.set(f"{{{ANDROID_NS}}}roundIcon", "@mipmap/ic_launcher_round")
    application.set(f"{{{ANDROID_NS}}}allowBackup", "false")
    application.set(f"{{{ANDROID_NS}}}fullBackupContent", "false")
    application.set(f"{{{ANDROID_NS}}}networkSecurityConfig", "@xml/nekopak_network_security")
    activity = application.find("activity")
    if activity is not None:
        activity.set(f"{{{ANDROID_NS}}}windowSoftInputMode", "adjustResize")
    ET.indent(tree, space="    ")
    tree.write(manifest_file, encoding="utf-8", xml_declaration=True)
    resources = project / "app/src/main/res"
    (resources / "xml").mkdir(parents=True, exist_ok=True)
    (resources / "xml/nekopak_network_security.xml").write_text('''<?xml version="1.0" encoding="utf-8"?>
<network-security-config>
    <base-config cleartextTrafficPermitted="false" />
    <domain-config cleartextTrafficPermitted="true">
        <domain includeSubdomains="false">127.0.0.1</domain>
        <domain includeSubdomains="false">localhost</domain>
        <domain includeSubdomains="false">tauri.localhost</domain>
        <domain includeSubdomains="false">ipc.localhost</domain>
    </domain-config>
</network-security-config>
''', encoding="utf-8", newline="\n")
    overlays = root / "src-tauri/android"
    copied = copy_kotlin(overlays, project / "app/src/main/java")
    tests = copy_kotlin(root / "src-tauri/androidTest", project / "app/src/androidTest/java")
    if (overlays / "res").is_dir():
        shutil.copytree(overlays / "res", resources, dirs_exist_ok=True)
    launcher = sync_android_icons(root, resources)
    proguard = project / "app/nekopak-jni.pro"
    proguard.write_text('''# Native Rust JNI looks up these application-owned classes by name.
-keep class io.nekopak.desktop.SessionVault { *; }
-keep class io.nekopak.desktop.AndroidFiles { *; }
''', encoding="utf-8", newline="\n")
    report = {"schema_version": 1, "toolchain": VERSIONS, "host_sources": copied,
              "launcher_icons": launcher,
              "instrumentation_sources": tests, "upstream_cli": "2.12.4",
              "upstream_commit": "993038c49185307dfc895ab7657d77ca0d5ed7c7",
              "source_handling": "Owned Kotlin and resources copied into ignored generated project; no secrets generated"}
    report["host_source_sha256"] = hashlib.sha256(b"".join(
        file.relative_to(root).as_posix().encode() + file.read_bytes()
        for base in [overlays, root / "src-tauri/androidTest"]
        for file in sorted(base.rglob("*")) if file.is_file()
    )).hexdigest()
    (project / "nekopak-project.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    version = json.loads((root / "package.json").read_text(encoding="utf-8"))["version"]
    android_config = json.loads((root / "src-tauri/tauri.android.conf.json").read_text(encoding="utf-8"))
    version_code = android_config["bundle"]["android"]["versionCode"]
    (project / "app/tauri.properties").write_text(
        f"# Generated by scripts/android_project.py\ntauri.android.versionName={version}\ntauri.android.versionCode={version_code}\n",
        encoding="utf-8", newline="\n")
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    command = parser.add_mutually_exclusive_group()
    command.add_argument("--capture-generated-icons", action="store_true", help="After tauri icon: preserve Android output in the committed resource directory.")
    command.add_argument("--generate-icons", action="store_true", help="Generate the SVG icon manifest and persist its Android output reproducibly.")
    options = parser.parse_args()
    if options.generate_icons:
        generate_icons()
    elif options.capture_generated_icons:
        capture_generated_icons()
    else:
        print(json.dumps(prepare(), indent=2))


if __name__ == "__main__":
    main()
