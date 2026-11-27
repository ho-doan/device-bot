#!/usr/bin/env python3
"""Verify Orbit action coverage against one UI Map folder and its action reports."""

import argparse
import json
from pathlib import Path

# Deduplicated app controls in it-01.yaml through it-09.yaml and tap_avatar.
# This is a read-only acceptance manifest, never an input to UI Map discovery.
BUTTONS = {
    "Demo": ("demo-button", ("Try demo workspace",)),
    "Sign in": ("sign-in-button", ("Sign in",)),
    "Password toggle": ("password-toggle-button", ("Show password", "Hide password")),
    "Forgot password": ("forgot-password-button", ("Forgot password?",)),
    "Try password reset": ("password-reset-button", ("Try password reset",)),
    "Back to sign in": ("back-to-sign-in-button", ("Back to sign in",)),
    "Account": (None, ("Account",)),
    "Overview": (None, ("Overview",)),
    "View your profile": ("view-profile-button", ("View your profile",)),
    "Avatar": ("account-avatar-button", ("Your account", "AL")),
    "Sign out": ("sign-out-button", ()),
    "Task 1": ("task-1-checkbox", ("Make space for a fresh start",)),
    "Task 2": ("task-2-checkbox", ("Choose your most important task",)),
    "Task 3": ("task-3-checkbox", ("Take a mindful break",)),
}
INPUTS = ("Login email", "Login password", "Reset email")
MIN_PERCENT = 95


def norm(label):
    return " ".join((label or "").split()).casefold()


def resource(control):
    return (control.get("resourceId") or "").split(":id/")[-1]


def matches(control, identifier, labels):
    actual_id = resource(control)
    if actual_id and identifier:
        return actual_id == identifier
    return norm(control.get("label")) in {norm(label) for label in labels}


def report_for(root, control):
    evidence = control.get("evidence")
    if not evidence:
        return None, set()
    path = (root / evidence["yaml"]).with_name("report.json").resolve()
    if not path.is_relative_to(root / "actions") or not path.is_file():
        return None, set()
    steps = json.loads(path.read_text())["steps"]
    return str(path.relative_to(root)), {
        step["action"] for step in steps if step["status"] == "passed"
    }


def score(root, snapshot):
    root = root.resolve()
    results = {key: [] for key in (*BUTTONS, *INPUTS)}
    for screen in snapshot["screens"]:
        controls = screen["controls"]
        login = any(matches(c, *BUTTONS["Sign in"]) for c in controls)
        reset = any(matches(c, *BUTTONS["Try password reset"]) for c in controls)
        for control in controls:
            if control["kind"] == "input":
                target = None
                if login != reset:
                    if matches(control, "email-field", ("Email address",)):
                        target = "Reset email" if reset else "Login email"
                    elif login and matches(control, "password-field", ("Password",)):
                        target = "Login password"
                targets = [target] if target else []
            else:
                targets = [
                    name for name, selectors in BUTTONS.items() if matches(control, *selectors)
                ]
            if not targets:
                continue
            report, actions = report_for(root, control)
            for target in targets:
                passed = (
                    "fill_field" in actions or {"focus_field", "type_text"} <= actions
                    if target in INPUTS
                    else "tap_element" in actions
                )
                results[target].append({
                    "screen": screen["id"],
                    "control": control["id"],
                    "passed": passed,
                    "status": control["status"],
                    "note": control.get("note"),
                    "report": report,
                })

    targets = {}
    for name, rows in results.items():
        if any(row["passed"] for row in rows):
            state = "covered"
        elif any(row["status"] == "failed" for row in rows):
            state = "attempted_failed"
        elif rows:
            state = "seen_not_attempted"
        else:
            state = "unseen"
        targets[name] = {"state": state, "observations": rows}
    count = sum(target["state"] == "covered" for target in targets.values())
    total = len(targets)
    return {
        "mapId": snapshot["id"],
        "deviceId": snapshot["deviceId"],
        "mapStatus": snapshot["status"],
        "count": count,
        "total": total,
        "percent": round(100 * count / total, 2),
        "passed": 100 * count >= MIN_PERCENT * total,
        "targets": targets,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("map_dir", type=Path, help="Directory with map.json and actions/")
    args = parser.parse_args()
    snapshot = json.loads((args.map_dir / "map.json").read_text())
    result = score(args.map_dir, snapshot)
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
