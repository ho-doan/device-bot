"""Run saved shell scenarios with MCP video evidence and preliminary reports."""
import base64
import json
import os
from pathlib import Path
import queue
import shutil
import signal
import subprocess
import sys
import threading
import time
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]


class RunnerInterrupted(BaseException):
    """Stop the suite on cancellation instead of treating it as a case error."""


class MCP:
    def __init__(self, directory):
        self.timeout = int(os.environ.get('MCP_TIMEOUT', '60'))
        if self.timeout <= 0:
            raise ValueError('MCP_TIMEOUT must be positive')
        self.log = (directory / 'mcp.jsonl').open('a')
        self.stderr = (directory / 'mcp-stderr.log').open('a')
        try:
            self.process = subprocess.Popen(
                [os.environ.get('DEVICE_BOT_BIN', 'device-bot')],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                stderr=self.stderr, text=True, bufsize=1,
            )
        except Exception:
            self.log.close()
            self.stderr.close()
            raise
        self.messages = queue.Queue()
        self.next_id = 1
        threading.Thread(target=self._read, daemon=True).start()
        try:
            self.request('initialize', {
                'protocolVersion': '2025-11-25', 'capabilities': {},
                'clientInfo': {'name': 'mobile-ci-evidence', 'version': '1.0'},
            })
            self.send({'jsonrpc': '2.0', 'method': 'notifications/initialized'})
        except Exception:
            self.close()
            raise

    def _read(self):
        for line in self.process.stdout:
            self.messages.put(line)
        self.messages.put(None)

    def send(self, value):
        self.process.stdin.write(json.dumps(value) + '\n')
        self.process.stdin.flush()

    def request(self, method, params):
        request_id = self.next_id
        self.next_id += 1
        self.send({'jsonrpc': '2.0', 'id': request_id, 'method': method, 'params': params})
        deadline = time.monotonic() + self.timeout
        while True:
            if time.monotonic() >= deadline:
                raise RuntimeError(f'MCP timeout: {method}')
            try:
                line = self.messages.get(timeout=max(0, deadline - time.monotonic()))
            except queue.Empty as error:
                raise RuntimeError(f'MCP timeout: {method}') from error
            if line is None:
                raise RuntimeError(f'MCP closed before response: {method}')
            self.log.write(line)
            self.log.flush()
            message = json.loads(line)
            if message.get('id') != request_id:
                continue
            if 'error' in message:
                raise RuntimeError(f'MCP protocol error: {message["error"]}')
            result = message['result']
            if result.get('isError'):
                raise RuntimeError(f'MCP tool error: {result.get("content")}')
            return result

    def tool(self, name, arguments):
        return self.request('tools/call', {'name': name, 'arguments': arguments})

    def close(self):
        try:
            self.process.stdin.close()
        except OSError:
            pass
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        self.process.stdout.close()
        self.log.close()
        self.stderr.close()


def collect_video(result, directory):
    prefix = 'Screen recording saved: '
    paths = [item.get('text', '')[len(prefix):].strip()
             for item in result.get('content', [])
             if item.get('text', '').startswith(prefix)]
    if len(paths) != 1 or not Path(paths[0]).is_file():
        raise RuntimeError('MCP did not return a local recorded video')
    target = directory / 'video.mp4'
    shutil.copyfile(paths[0], target)
    verify_video(target)


def verify_video(target):
    if not target.is_file():
        raise RuntimeError('Scenario did not save video.mp4')
    probe = subprocess.run([
        'ffprobe', '-v', 'error', '-show_entries',
        'stream=codec_type:format=duration', '-of', 'json', str(target),
    ], capture_output=True, text=True, timeout=30, check=True)
    info = json.loads(probe.stdout)
    if (not any(s.get('codec_type') == 'video' for s in info.get('streams', []))
            or float(info.get('format', {}).get('duration', 0)) <= 0):
        raise RuntimeError('Video evidence is empty or has no video stream')


def run_shell(script, env, directory, timeout):
    with (directory / 'scenario.log').open('w') as log:
        process = subprocess.Popen(['bash', str(script)], cwd=ROOT, env=env,
                                   stdout=log, stderr=subprocess.STDOUT,
                                   start_new_session=True)
        try:
            return process.wait(timeout=timeout)
        finally:
            # Stop descendants too: a timed-out shell may leave an MCP process.
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            if process.poll() is None:
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
            # The shell may have exited while a descendant ignored SIGTERM.
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass


def run_case(script, directory, timeout):
    directory.mkdir(parents=True)
    result = {'case': script.stem, 'script': str(script.relative_to(ROOT)),
              'preliminary': True, 'qa_status': 'PENDING', 'status': 'ERROR',
              'exit_code': None, 'issues': []}
    mcp = None
    recording = False
    args = {'deviceId': os.environ['DEVICE_ID']}
    try:
        if os.environ.get('SCENARIO_RECORDS_VIDEO') != 'true':
            mcp = MCP(directory)
            recording = True
            mcp.tool('mobile_start_screen_recording', args)
        env = dict(os.environ, EVIDENCE_DIR=str(directory), CASE_ID=script.stem)
        result['exit_code'] = run_shell(script, env, directory, timeout)
        if result['exit_code'] != 0:
            result['status'] = 'FAIL'
            result['issues'].append(f'Scenario exited {result["exit_code"]}')
        else:
            # Explicit result required; exit zero alone is not a coverage claim.
            declared = json.loads((directory / 'result.json').read_text())
            if declared.get('status') not in ('PASS', 'PARTIAL'):
                raise ValueError('result.json must declare PASS or PARTIAL')
            limits = declared.get('limitations', [])
            if not isinstance(limits, list) or not all(isinstance(v, str) for v in limits):
                raise ValueError('limitations must be a list of strings')
            if declared['status'] == 'PARTIAL' and not limits:
                raise ValueError('PARTIAL must list the uncovered assertions')
            if declared['status'] == 'PASS' and limits:
                raise ValueError('PASS cannot contain coverage limitations')
            result.update(status=declared['status'], limitations=limits)
    except Exception as error:
        result['issues'].append(str(error))
    finally:
        if mcp:
            if recording:
                try:
                    collect_video(mcp.tool('mobile_stop_screen_recording', args), directory)
                    result['video'] = str((directory / 'video.mp4').relative_to(ROOT))
                except Exception as error:
                    result['issues'].append(f'Video evidence error: {error}')
                    result['status'] = 'ERROR'
            try:
                shot = mcp.tool('mobile_take_screenshot', args)
                for item in shot.get('content', []):
                    if item.get('type') == 'image' and item.get('mimeType') == 'image/png':
                        (directory / 'screen.png').write_bytes(base64.b64decode(item['data']))
                        break
            except Exception as error:
                result['screenshot_warning'] = str(error)
            mcp.close()
        elif os.environ.get('SCENARIO_RECORDS_VIDEO') == 'true':
            try:
                verify_video(directory / 'video.mp4')
                result['video'] = str((directory / 'video.mp4').relative_to(ROOT))
            except Exception as error:
                result['issues'].append(f'Video evidence error: {error}')
                result['status'] = 'ERROR'
    (directory / 'preliminary.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


def write_reports(results, output):
    summary = {'preliminary': True, 'qa_status': 'PENDING', 'cases': results,
               'commit': os.environ.get('CI_COMMIT_SHA', os.environ.get('GITHUB_SHA', 'local')),
               'platform': os.environ.get('PLATFORM'),
               'app_id': os.environ.get('APP_ID'),
               'device_id': os.environ.get('DEVICE_ID')}
    (output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    suite = ET.Element('testsuite', name='mobile-preliminary', tests=str(len(results)),
                       failures=str(sum(r['status'] == 'FAIL' for r in results)),
                       errors=str(sum(r['status'] == 'ERROR' for r in results)),
                       skipped=str(sum(r['status'] == 'PARTIAL' for r in results)))
    for result in results:
        case = ET.SubElement(suite, 'testcase', name=result['case'], classname='mobile')
        status = result['status']
        if status in ('FAIL', 'ERROR'):
            node = ET.SubElement(case, 'failure' if status == 'FAIL' else 'error',
                                 message='; '.join(result['issues']))
            node.text = '\n'.join(result['issues'])
        elif status == 'PARTIAL':
            ET.SubElement(case, 'skipped', message='Uncovered assertions: ' +
                          '; '.join(result['limitations']))
        ET.SubElement(case, 'system-out').text = json.dumps(result)
    ET.ElementTree(suite).write(output / 'junit.xml', encoding='utf-8', xml_declaration=True)
    lines = ['# Preliminary automation results', '',
             '**QA review: PENDING. CI success is not acceptance.**', '']
    for result in results:
        lines.append(f'- {result["case"]}: {result["status"]}')
        lines.extend(f'  - {message}' for message in result.get('issues', []))
        lines.extend(f'  - Not covered: {message}' for message in result.get('limitations', []))
    (output / 'QA_REVIEW.md').write_text('\n'.join(lines) + '\n')


def main():
    output = ROOT / 'artifacts'
    output.mkdir(exist_ok=True)
    results = []
    try:
        for key in ('DEVICE_ID', 'APP_ID'):
            if not os.environ.get(key):
                raise ValueError(f'Set {key} in CI variables')
        if os.environ.get('PLATFORM') not in ('android', 'ios-sim'):
            raise ValueError('PLATFORM must be android or ios-sim')
        timeout = int(os.environ.get('SCENARIO_TIMEOUT', '120'))
        if timeout <= 0:
            raise ValueError('SCENARIO_TIMEOUT must be positive')
        for executable in ('bash', 'ffprobe', os.environ.get('DEVICE_BOT_BIN', 'device-bot')):
            if not shutil.which(executable):
                raise ValueError(f'Missing runner dependency: {executable}')
        scripts = sorted(path for path in (ROOT / 'automation_test').glob('*.sh')
                         if path.is_file())
        if not scripts:
            raise ValueError('No project scenarios yet: add validated automation_test/*.sh')
        for script in scripts:
            print(f'Running {script.name}; QA review remains pending', flush=True)
            results.append(run_case(script, output / script.stem, timeout))
    except (Exception, RunnerInterrupted) as error:
        results.append({'case': 'suite-setup', 'status': 'ERROR', 'preliminary': True,
                        'qa_status': 'PENDING', 'issues': [str(error)]})
    finally:
        write_reports(results, output)
    return int(any(result['status'] in ('FAIL', 'ERROR') for result in results))


if __name__ == '__main__':
    # Raising through run_shell/run_case still executes recording/report cleanup.
    def interrupted(signum, frame):
        raise RunnerInterrupted(f'Runner interrupted by signal {signum}')
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    sys.exit(main())
