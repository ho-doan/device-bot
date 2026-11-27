import json
from pathlib import Path
import tempfile
import unittest

from ui_map_coverage import BUTTONS, score


class CoverageGateTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.map = {
            "id": "one-run",
            "deviceId": "emulator-fixture",
            "status": "completed",
            "screens": [],
        }

    def control(self, name, identifier, kind="button", actions=(), status="observed"):
        self.serial = getattr(self, "serial", 0) + 1
        key = f"c{self.serial}"
        row = {"id": key, "label": name, "resourceId": identifier, "kind": kind, "status": status}
        if actions:
            directory = self.root / "actions" / key
            directory.mkdir(parents=True)
            (directory / "report.json").write_text(json.dumps({
                "steps": [{"action": action, "status": state} for action, state in actions],
            }))
            row["evidence"] = {"yaml": f"actions/{key}/scenario.yaml"}
        return row

    def screen(self, *controls):
        self.map["screens"].append({"id": f"s{len(self.map['screens']) + 1}", "controls": controls})

    def test_gate_requires_each_unique_control_and_separates_equal_ids_by_screen(self):
        tap = [("tap_element", "passed")]
        fill = [("focus_field", "passed"), ("type_text", "passed")]
        buttons = {
            key: self.control(labels[0] if labels else key, identifier, actions=tap)
            for key, (identifier, labels) in BUTTONS.items()
        }
        self.screen(
            buttons.pop("Sign in"),
            self.control("Email address", "email-field", "input", fill),
            self.control("Password", "password-field", "input", fill),
            self.control("Hide password", "password-toggle-button", actions=tap),
            *(buttons[key] for key in list(buttons) if key != "Try password reset"),
        )
        self.screen(
            buttons["Try password reset"],
            self.control("Email address", "email-field", "input", [("assert_value", "passed")]),
        )
        result = score(self.root, self.map)
        self.assertEqual(result["count"], 16)
        self.assertFalse(result["passed"])
        self.assertEqual(result["targets"]["Reset email"]["state"], "seen_not_attempted")

        self.screen(
            buttons["Try password reset"],
            self.control("Email address", "email-field", "input", fill),
        )
        result = score(self.root, self.map)
        self.assertEqual(result["count"], 17)
        self.assertTrue(result["passed"])

    def test_seen_and_wrong_context_do_not_cover_an_action(self):
        self.screen(
            self.control("Sign out", "rail-sign-out-button", actions=[("tap_element", "passed")]),
            self.control("Sign out", None, actions=[("tap_element", "passed")]),
            self.control("Sign in", "sign-in-button", actions=[("tap_element", "failed")], status="failed"),
            self.control("Try demo workspace", "demo-button", status="pending"),
        )
        states = score(self.root, self.map)["targets"]
        self.assertEqual(states["Sign out"]["state"], "unseen")
        self.assertEqual(states["Sign in"]["state"], "attempted_failed")
        self.assertEqual(states["Demo"]["state"], "seen_not_attempted")


if __name__ == "__main__":
    unittest.main()
