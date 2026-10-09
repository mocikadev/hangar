#!/usr/bin/env python3
"""Empty-home native desktop smoke. Requires python3-gi/gir1.2-atspi-2.0 and a display.

Runs on a private D-Bus session, never opens a browser or copies real credentials.
"""
import argparse
import os
import pathlib
import subprocess
import sys
import tempfile
import time

root = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--binary', type=pathlib.Path, default=root / 'target/debug/hangar-desktop')
parser.add_argument('--session-child', action='store_true', help=argparse.SUPPRESS)
args = parser.parse_args()
binary = args.binary.resolve()
backend = os.environ.get('GDK_BACKEND', 'wayland')
output = root / 'build/linux-qa' / backend
output.mkdir(parents=True, exist_ok=True)
if not args.session_child:
    with tempfile.TemporaryDirectory(prefix='hangar-desktop-smoke-') as home:
        env = os.environ.copy()
        wayland = env.get('WAYLAND_DISPLAY', '')
        if wayland and not os.path.isabs(wayland):
            env['WAYLAND_DISPLAY'] = str(pathlib.Path(env.get('XDG_RUNTIME_DIR', '/run/user/' + str(os.getuid()))) / wayland)
        runtime = pathlib.Path(home, 'runtime')
        runtime.mkdir(mode=0o700)
        env['XDG_RUNTIME_DIR'] = str(runtime)
        env.update(HOME=home, CODEX_HOME=f'{home}/.codex', HANGAR_TEST_HOME=home,
                   HANGAR_TEST_TOKEN_ENDPOINT='http://127.0.0.1:9/token',
                   HANGAR_TEST_USAGE_ENDPOINT='http://127.0.0.1:9/usage', GTK_A11Y='atspi',
                   GDK_BACKEND=backend, GIO_USE_VFS='local',
                   XDG_CONFIG_HOME=f'{home}/.config', XDG_DATA_HOME=f'{home}/.local/share')
        pathlib.Path(home, '.codex').mkdir(mode=0o700)
        result = subprocess.call(['dbus-run-session', '--', sys.executable, __file__,
                                  '--session-child', '--binary', str(binary)], env=env)
        # The private portal can leave its own FUSE documents mount after bus shutdown.
        for name in ('doc', 'gvfs'):
            mount = runtime / name
            subprocess.run(['fusermount3', '-u', '-z', str(mount)],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        raise SystemExit(result)

import gi  # Optional QA-only system dependency, not an application dependency.
gi.require_version('Atspi', '2.0')
from gi.repository import Atspi, Gio, GLib


def tick():
    while GLib.MainContext.default().iteration(False):
        pass
    time.sleep(.1)

def walk(node):
    if node is None:
        return []
    rows = []
    try:
        node.clear_cache()
        rows.append((node.get_role_name(), node.get_name(), node))
        for index in range(node.get_child_count()):
            rows.extend(walk(node.get_child_at_index(index)))
    except GLib.Error:
        pass
    return rows

def tree():
    desktop = Atspi.get_desktop(0)
    rows = []
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app is not None and app.get_name() in ('hangar-desktop', 'Hangar'):
            rows.extend(walk(app))
    return rows

def wait_for(predicate, label):
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        tick()
        rows = tree()
        if predicate(rows):
            (output / f'{label}.txt').write_text('\n'.join(f'{role}: {name}' for role, name, _ in rows))
            return rows
        if process.poll() is not None:
            raise AssertionError(f'application exited before {label}')
    (output / f'{label}-timeout.txt').write_text('\n'.join(f'{role}: {name}' for role, name, _ in tree()))
    raise AssertionError(f'timed out waiting for {label}')


with (output / 'app.log').open('w') as log:
    process = subprocess.Popen([str(binary)], stdout=log, stderr=log)
    try:
        bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        def activate(name):
            bus.call_sync('dev.mocika.hangar', '/dev/mocika/hangar', 'org.gtk.Actions',
                          'Activate', GLib.Variant('(sava{sv})', (name, [], {})), None,
                          Gio.DBusCallFlags.NONE, 5000, None)
        wait_for(lambda rows: any(name == '添加你的第一个账号' for _, name, _ in rows), 'empty')
        for _ in range(10): tick()
        subprocess.run([str(binary)], timeout=10, check=True, stdout=log, stderr=log)
        for _ in range(10): tick()
        assert sum(role == 'frame' for role, _, _ in tree()) == 1
        activate('quit')
        assert process.wait(timeout=15) == 0
        assert not pathlib.Path(os.environ['CODEX_HOME'], 'auth.json').exists()
        print(f'{backend}: empty window, single instance and explicit quit passed')
    finally:
        if process.poll() is None:
            activate('quit')
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.terminate()  # Only this test's empty-home child, never a user process.
                process.wait(timeout=5)
