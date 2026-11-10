import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "server/crates/robot-ios-real/xctest-runner/configure_signing.py"
spec = importlib.util.spec_from_file_location("configure_signing", SCRIPT)
signing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(signing)


class SigningTests(unittest.TestCase):
    def test_team_comes_from_certificate_ou_matching_a_valid_identity(self):
        fingerprint = "A" * 40
        pem = "-----BEGIN CERTIFICATE-----\nfixture\n-----END CERTIFICATE-----"
        def fake_run(*args, **kwargs):
            if args[1] == "find-identity":
                return f'1) {fingerprint} "Apple Development: Example (PERSON1234)"'
            if args[1] == "find-certificate":
                return pem + "\n" + pem
            fake_run.calls += 1
            sha = ":".join(["AA" if fake_run.calls == 1 else "BB"] * 20)
            return f"sha1 Fingerprint={sha}\nsubject=\n    OU=TEAM123456\n"
        fake_run.calls = 0
        with patch.object(signing, "run", side_effect=fake_run):
            self.assertEqual(signing.installed_teams(), {"TEAM123456"})

    def test_replaces_every_configuration_and_is_idempotent(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory) / "project.pbxproj"
            project.write_text('DEVELOPMENT_TEAM = OLDTEAM123;\nDEVELOPMENT_TEAM = "OLDTEAM123";\nOTHER = keep;\n')
            signing.update_project(project, {"TEAM123456"})
            self.assertEqual(project.read_text(), 'DEVELOPMENT_TEAM = TEAM123456;\nDEVELOPMENT_TEAM = TEAM123456;\nOTHER = keep;\n')
            changed = project.stat().st_mtime_ns
            signing.update_project(project, {"TEAM123456"})
            self.assertEqual(project.stat().st_mtime_ns, changed)

    def test_multiple_certificates_keep_a_matching_project_team(self):
        self.assertEqual(signing.select_team({"TEAM123456", "TEAM654321"}, None,
                                            "DEVELOPMENT_TEAM = TEAM654321;"), "TEAM654321")

    def test_ambiguous_or_missing_identity_never_changes_project(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory) / "project.pbxproj"
            original = "DEVELOPMENT_TEAM = OLDTEAM123;"
            project.write_text(original)
            for teams, requested in [(set(), None), ({"TEAM123456", "TEAM654321"}, None),
                                     ({"TEAM123456"}, "TEAM654321")]:
                with self.assertRaises(ValueError):
                    signing.update_project(project, teams, requested)
                self.assertEqual(project.read_text(), original)

    def test_print_only_does_not_replace(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory) / "project.pbxproj"
            project.write_text("DEVELOPMENT_TEAM = OLDTEAM123;")
            self.assertEqual(signing.update_project(project, {"TEAM123456"}, print_only=True), "TEAM123456")
            self.assertEqual(project.read_text(), "DEVELOPMENT_TEAM = OLDTEAM123;")


if __name__ == "__main__":
    unittest.main()
