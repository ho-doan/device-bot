"""Check CI Python and scenario shell syntax without starting a device."""
import ast
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
for directory in (ROOT / 'ci', ROOT / 'automation_test'):
    for path in sorted(directory.rglob('*.py')):
        ast.parse(path.read_text(), filename=str(path))
    for path in sorted(directory.rglob('*.sh')):
        subprocess.run(['bash', '-n', str(path)], check=True)
print('Static checks passed. Device scenarios: NOT RUN. QA review: PENDING.')
