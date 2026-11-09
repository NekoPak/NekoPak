"""Apply the checked-in Android host to Tauri's disposable generated project."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
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
    proguard = project / "app/nekopak-jni.pro"
    proguard.write_text('''# Native Rust JNI looks up these application-owned classes by name.
-keep class io.nekopak.desktop.SessionVault { *; }
-keep class io.nekopak.desktop.AndroidFiles { *; }
''', encoding="utf-8", newline="\n")
    report = {"schema_version": 1, "toolchain": VERSIONS, "host_sources": copied,
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
    parser.parse_args()
    print(json.dumps(prepare(), indent=2))


if __name__ == "__main__":
    main()
