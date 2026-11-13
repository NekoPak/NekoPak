"""Validate a tag, manual run, or merged release PR before exposing its version."""
import json
import os
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]


def resolve_version(version, event_name, ref, event, repository, requested=""):
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("The project release version must be a numeric semantic version.")
    if event_name == "pull_request":
        pr = event.get("pull_request", {})
        head = pr.get("head", {})
        if (pr.get("merged") is not True
                or pr.get("base", {}).get("ref") != "main"
                or head.get("repo", {}).get("full_name") != repository
                or head.get("ref") != f"codex/release-{version}"):
            raise ValueError("Only a merged release PR from this repository may publish.")
        return version
    if event_name == "push" and ref.startswith("refs/tags/"):
        label = ref.removeprefix("refs/tags/")
        # Keep the existing strict parity-tag path; the workflow's parity
        # checks and the preview publisher's identity checks still apply.
        if re.fullmatch(r"parity-[A-Za-z0-9._-]+", label):
            return label
    elif event_name == "workflow_dispatch":
        label = requested or version
    else:
        raise ValueError("Unsupported release trigger.")
    if label not in {version, f"v{version}"}:
        raise ValueError("The requested release label does not match the checked out source.")
    return label


def main():
    version = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))["version"]
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
    label = resolve_version(version, os.environ["GITHUB_EVENT_NAME"], os.environ["GITHUB_REF"],
                            event, os.environ["GITHUB_REPOSITORY"],
                            os.environ.get("NEKOPAK_RELEASE_VERSION_INPUT", ""))
    with Path(os.environ["GITHUB_ENV"]).open("a", encoding="utf-8") as output:
        output.write(f"RELEASE_VERSION={label}\n")
    print(f"Validated release context for {label}.")


if __name__ == "__main__":
    main()
