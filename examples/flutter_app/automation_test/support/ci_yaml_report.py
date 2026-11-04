"""Publish a setup error when CI exits before the native YAML runner can report."""
from pathlib import Path
import sys
import xml.etree.ElementTree as ET


def ensure_setup_report(directory):
    directory = Path(directory)
    if any(directory.glob('*/junit.xml')):
        return
    target = directory / 'setup'
    target.mkdir(parents=True, exist_ok=True)
    suite = ET.Element('testsuite', name='mobile-yaml-setup', tests='1', errors='1')
    case = ET.SubElement(suite, 'testcase', name='setup')
    ET.SubElement(case, 'error', message='CI setup ended before robot-flow produced a report; inspect the job log.')
    ET.ElementTree(suite).write(target / 'junit.xml', encoding='utf-8', xml_declaration=True)


if __name__ == '__main__':
    ensure_setup_report(sys.argv[1])
