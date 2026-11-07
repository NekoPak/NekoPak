"""Validate the tracked functional baseline; --release rejects unfinished Windows parity."""
import argparse
import json
from datetime import date, datetime, timedelta, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
STATES = {'planned', 'partial', 'blocked', 'unverified', 'verified'}
REQUIRED_WINDOWS = set('auth_accounts file_browser_index file_mutations batch_rename_undo duplicates download_full transfer_queue upload cloud_tasks quota media_representations segment_local segment_remote segment_ts segment_tasks player_subtitles gallery_feed share_preview archive_references archive_browser local_folder_mount workspace_navigation preferences_i18n desktop_distribution'.split())

def check(manifest, release=False):
    errors = []
    ids = set()
    baseline = manifest.get('baseline', {})
    commit = baseline.get('commit', '')
    if manifest.get('schema_version') != 1 or len(commit) != 40 or any(c not in '0123456789abcdef' for c in commit):
        errors.append('A versioned manifest and pinned baseline commit are required.')
    try:
        date.fromisoformat(baseline.get('reviewed_at', ''))
    except (ValueError, TypeError):
        errors.append('Baseline must contain a valid reviewed_at date.')
    if baseline.get('latest_stable_review_required_before_release') is not True:
        errors.append('Latest-stable upstream review cannot be disabled.')
    if release:
        review = manifest.get('release_review', {})
        evidence = review.get('evidence', '')
        if review.get('baseline_commit') != commit or not review.get('reviewer') or not review.get('release_candidate'):
            errors.append('A named release-candidate review of the selected baseline is required.')
        try:
            reviewed = date.fromisoformat(review.get('reviewed_at', ''))
            today = datetime.now(timezone(timedelta(hours=8))).date()
            if not 0 <= (today - reviewed).days <= 7:
                errors.append('Latest-stable release review is older than 7 days or in the future.')
        except (ValueError, TypeError):
            errors.append('Release review date is required.')
        evidence_path = (ROOT / evidence).resolve()
        if not evidence or not evidence_path.is_relative_to(ROOT) or not evidence_path.is_file():
            errors.append('Latest-stable release review evidence file is required.')
    for feature in manifest.get('features', []):
        name = feature.get('id', '')
        if not name or name in ids:
            errors.append(f'Duplicate/missing feature ID: {name}')
        ids.add(name)
        if name in REQUIRED_WINDOWS and feature.get('mandatory_windows') is not True:
            errors.append(f'Required Windows feature was demoted: {name}')
        if feature.get('status') not in STATES or not feature.get('acceptance') or not isinstance(feature.get('mandatory_windows'), bool):
            errors.append(f'Invalid feature schema: {name}')
        for path in feature.get('evidence', []):
            resolved = (ROOT / path).resolve()
            if not resolved.is_relative_to(ROOT) or not resolved.is_file():
                errors.append(f'Missing or outside-workspace evidence: {name}: {path}')
        if feature.get('status') == 'verified' and not feature.get('evidence'):
            errors.append(f'Verified feature has no evidence: {name}')
        if release and feature.get('mandatory_windows') and feature.get('status') != 'verified':
            errors.append(f'Unfinished required parity: {name} ({feature.get("status")})')
    for required in sorted(REQUIRED_WINDOWS):
        if required not in ids:
            errors.append(f'Required Windows parity item removed: {required}')
    return errors

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--release', action='store_true')
    args = parser.parse_args()
    manifest = json.loads((ROOT / 'docs/parity-manifest.json').read_text(encoding='utf-8'))
    errors = check(manifest, args.release)
    required = [f for f in manifest['features'] if f['mandatory_windows']]
    verified = sum(f['status'] == 'verified' for f in required)
    print(f'Windows functional parity: {verified}/{len(required)} required groups verified; previews are not parity releases.')
    for message in errors:
        print(message)
    raise SystemExit(1 if errors else 0)

if __name__ == '__main__':
    main()
