#!/usr/bin/env python3
"""Select an installed iOS development identity and update Xcode signing teams."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


TEAM_SETTING = re.compile(r'(\bDEVELOPMENT_TEAM\s*=\s*)(?:"[^"]*"|[^;\s]+)(\s*;)')


def run(*args, input=None):
    return subprocess.run(args, input=input, text=True, capture_output=True,
                          check=True, timeout=15).stdout


def installed_teams(identity_hint=None):
    identities = run("security", "find-identity", "-v", "-p", "codesigning")
    teams = set()
    for fingerprint, name in re.findall(r'\b([A-Fa-f0-9]{40})\s+"([^"]+)"', identities):
        if not name.startswith(("Apple Development:", "iPhone Developer:")):
            continue
        if identity_hint and identity_hint not in name:
            continue
        certificates = run("security", "find-certificate", "-a", "-c", name, "-p")
        for pem in re.findall(r"-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----",
                              certificates, re.S):
            info = run("openssl", "x509", "-noout", "-fingerprint", "-sha1",
                       "-subject", "-nameopt", "sep_multiline,sname", input=pem)
            sha = re.search(r"(?i)fingerprint=([A-F0-9:]+)", info)
            team = re.search(r"(?m)^\s*OU\s*=\s*([A-Z0-9]{10})\s*$", info)
            if sha and team and sha[1].replace(":", "").upper() == fingerprint.upper():
                teams.add(team[1])
    return teams


def select_team(teams, requested, contents):
    if requested:
        if requested not in teams:
            raise ValueError(f"No valid iOS development signing identity for team {requested}")
        return requested
    if len(teams) == 1:
        return next(iter(teams))
    current = {match.strip('"') for match in re.findall(
        r'\bDEVELOPMENT_TEAM\s*=\s*("[^"]*"|[^;\s]+)\s*;', contents)}
    matching = current & teams
    if len(matching) == 1:
        return next(iter(matching))
    if not teams:
        raise ValueError("No valid iOS development signing identity; configure Signing in Xcode first")
    raise ValueError("Multiple signing teams: " + ", ".join(sorted(teams))
                     + "; select DEVICE_BOT_IOS_TEAM")


def update_project(project, teams, requested=None, print_only=False):
    path = Path(project)
    if path.is_dir():
        path /= "project.pbxproj"
    contents = path.read_text()
    team = select_team(teams, requested, contents)
    updated, count = TEAM_SETTING.subn(lambda match: match[1] + team + match[2], contents)
    if not count:
        raise ValueError(f"No DEVELOPMENT_TEAM settings in {path}")
    if not print_only and updated != contents:
        # Resolve symlinks and replace atomically, preserving the project's permissions.
        path = path.resolve()
        temporary = None
        try:
            with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as file:
                temporary = Path(file.name)
                file.write(updated)
            temporary.chmod(path.stat().st_mode & 0o777)
            temporary.replace(path)
        finally:
            if temporary and temporary.exists():
                temporary.unlink()
        print(f"Updated {count} DEVELOPMENT_TEAM settings -> {team}", file=sys.stderr)
    return team


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project", type=Path, default=Path(__file__).parent / "MobileMcpRealRunner.xcodeproj")
    parser.add_argument("--team-id", default=os.environ.get("DEVICE_BOT_IOS_TEAM"))
    parser.add_argument("--identity", default=os.environ.get("DEVICE_BOT_IOS_SIGNING_IDENTITY"),
                        help="Optional development certificate name/email filter")
    parser.add_argument("--print-only", action="store_true", help="Resolve the team without changing the project")
    args = parser.parse_args()
    if args.team_id and not re.fullmatch(r"[A-Z0-9]{10}", args.team_id):
        parser.error("Team ID must contain exactly 10 uppercase letters/digits")
    try:
        print(update_project(args.project, installed_teams(args.identity), args.team_id, args.print_only))
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"iOS signing setup failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
