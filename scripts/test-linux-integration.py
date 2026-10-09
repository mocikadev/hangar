#!/usr/bin/env python3
"""Credential-free SNI lifecycle QA, with a private fake host or the real desktop host.

Build hangar-linux first. --real-session uses a unique QA app ID and empty config,
never changes desktop settings. Default uses a private D-Bus host to test host loss.
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
parser.add_argument('--real-session', action='store_true')
parser.add_argument('--child', action='store_true', help=argparse.SUPPRESS)
args = parser.parse_args()
if not args.child:
    with tempfile.TemporaryDirectory(prefix='hangar-integration-') as home:
        env = os.environ.copy()
        env.update(HOME=home, CODEX_HOME=home+'/.codex', HANGAR_TEST_HOME=home,
                   XDG_CONFIG_HOME=home+'/config', XDG_DATA_HOME=home+'/data', XDG_CACHE_HOME=home+'/cache',
                   HANGAR_TEST_APP_ID=f'dev.mocika.hangar.QaIntegration{os.getpid()}',
                   HANGAR_TEST_TOKEN_ENDPOINT='http://127.0.0.1:9/token',
                   HANGAR_TEST_USAGE_ENDPOINT='http://127.0.0.1:9/usage', GIO_USE_VFS='local')
        if not args.real_session:
            wayland = env.get('WAYLAND_DISPLAY', '')
            if wayland and not wayland.startswith('/'):
                env['WAYLAND_DISPLAY'] = str(pathlib.Path(env['XDG_RUNTIME_DIR'], wayland))
            env['XDG_RUNTIME_DIR'] = home+'/runtime'
            pathlib.Path(env['XDG_RUNTIME_DIR']).mkdir(mode=0o700)
        command = ['/usr/bin/python3', __file__, '--child']
        command = command+['--real-session'] if args.real_session else ['dbus-run-session', '--']+command
        try:
            result = subprocess.run(command, env=env, timeout=90)
        finally:
            if not args.real_session:
                for name in ('doc', 'gvfs'):
                    subprocess.run(['fusermount3', '-u', '-z', home+'/runtime/'+name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        sys.exit(result.returncode)

import gi
gi.require_version('Atspi', '2.0')
from gi.repository import Atspi, Gio, GLib
bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
watcher_name = 'org.kde.StatusNotifierWatcher'
watcher_path = '/StatusNotifierWatcher'
registered = []
owner = None

def tick():
    while GLib.MainContext.default().iteration(False):
        pass
    time.sleep(.03)

def wait(predicate, label):
    deadline = time.monotonic()+15
    while time.monotonic() < deadline:
        tick()
        if predicate():
            return
    raise AssertionError('timed out: '+label)

def call(destination, path, interface, method, parameters=None):
    return bus.call_sync(destination, path, interface, method, parameters, None, Gio.DBusCallFlags.NONE, 4000, None).unpack()

def prop(destination, path, interface, name):
    return call(destination, path, 'org.freedesktop.DBus.Properties', 'Get', GLib.Variant('(ss)', (interface, name)))[0]

if not args.real_session:
    xml = '''<node><interface name="org.kde.StatusNotifierWatcher">
    <method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
    <property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
    <property name="RegisteredStatusNotifierItems" type="as" access="read"/>
    <property name="ProtocolVersion" type="i" access="read"/>
    <signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
    </interface></node>'''
    def method(connection, sender, path, interface, name, params, invocation):
        value = params.unpack()[0]
        item = sender+value if value.startswith('/') else value+'/StatusNotifierItem'
        registered.append(item)
        invocation.return_value(GLib.Variant('()', ()))
        connection.emit_signal(None, watcher_path, watcher_name, 'StatusNotifierItemRegistered', GLib.Variant('(s)', (item,)))
    def get_property(connection, sender, path, interface, name):
        return {'IsStatusNotifierHostRegistered': GLib.Variant('b', True),
                'RegisteredStatusNotifierItems': GLib.Variant('as', registered),
                'ProtocolVersion': GLib.Variant('i', 0)}[name]
    bus.register_object(watcher_path, Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0], method, get_property, None)
    owner = Gio.bus_own_name_on_connection(bus, watcher_name, Gio.BusNameOwnerFlags.NONE, None, None)
    wait(lambda: call('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'NameHasOwner', GLib.Variant('(s)', (watcher_name,)))[0], 'mock watcher')
else:
    assert prop(watcher_name, watcher_path, watcher_name, 'IsStatusNotifierHostRegistered')

app_id = os.environ['HANGAR_TEST_APP_ID']
app_path = '/'+app_id.replace('.', '/')
logs = root/'build/linux-qa'
logs.mkdir(parents=True, exist_ok=True)
mode = 'real' if args.real_session else 'private'
log = (logs/f'integration-{mode}-app.log').open('w')
process = subprocess.Popen([str(root/'target/debug/hangar-desktop')], stdout=log, stderr=log)

def action(name):
    return call(app_id, app_path, 'org.gtk.Actions', 'Activate', GLib.Variant('(sava{sv})', (name, [], {})))

def frames():
    result = []
    desktop = Atspi.get_desktop(0)
    for i in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(i)
        if app and app.get_process_id() == process.pid:
            app.clear_cache()
            for j in range(app.get_child_count()):
                node = app.get_child_at_index(j)
                if node and node.get_role_name() == 'frame':
                    node.clear_cache()
                    result.append(node)
    return result

def showing():
    try:
        return any(n.get_state_set().contains(Atspi.StateType.SHOWING) for n in frames())
    except GLib.Error:
        return False

def own_item():
    # Match D-Bus peer PID, never infer ownership from a title shared with real instances.
    items = prop(watcher_name, watcher_path, watcher_name, 'RegisteredStatusNotifierItems') if args.real_session else registered
    for item in items:
        destination, _, path = item.partition('/')
        try:
            pid = call('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'GetConnectionUnixProcessID', GLib.Variant('(s)', (destination,)))[0]
            if pid == process.pid:
                return destination, '/'+path if path else '/StatusNotifierItem'
        except GLib.Error:
            pass
    return None

def menu_entries(destination, path):
    _, layout = call(destination, path, 'com.canonical.dbusmenu', 'GetLayout', GLib.Variant('(iias)', (0, -1, ['label', 'enabled'])))
    def walk(node):
        if isinstance(node, GLib.Variant): node = node.unpack()
        ident, properties, children = node
        yield ident, properties
        for child in children: yield from walk(child)
    return list(walk(layout))

def menu_click(destination, path, label):
    entry = next((i for i, p in menu_entries(destination, path) if p.get('label') == label), None)
    assert entry is not None, label
    call(destination, path, 'com.canonical.dbusmenu', 'Event', GLib.Variant('(isvu)', (entry, 'clicked', GLib.Variant('i', 0), 0)))

try:
    wait(showing, 'empty window')
    wait(lambda: own_item() is not None, 'SNI registration')
    destination, path = own_item()
    assert prop(destination, path, 'org.kde.StatusNotifierItem', 'Title') == 'Hangar'
    pixmaps = prop(destination, path, 'org.kde.StatusNotifierItem', 'IconPixmap')
    assert pixmaps and list(pixmaps[0][0:2]) == [32, 32]
    assert len(pixmaps[0][2]) == 32*32*4
    menu_path = prop(destination, path, 'org.kde.StatusNotifierItem', 'Menu')
    wait(lambda: any(p.get('label') == '刷新周额度' for _, p in menu_entries(destination, menu_path)), 'idle menu')
    for _ in range(20): tick()
    action('close')
    wait(lambda: not showing(), 'close hides')
    assert process.poll() is None
    menu_click(destination, menu_path, '显示 Hangar')
    wait(showing, 'menu restores')
    wait(lambda: any(p.get('label') == '刷新周额度' for _, p in menu_entries(destination, menu_path)), 'refresh enabled')
    menu_click(destination, menu_path, '刷新周额度')
    for _ in range(20): tick()
    if not args.real_session:
        action('close')
        wait(lambda: not showing(), 'hide before host loss')
        Gio.bus_unown_name(owner)
        owner = None
        wait(showing, 'host loss restores window')
        registered.clear()
        owner = Gio.bus_own_name_on_connection(bus, watcher_name, Gio.BusNameOwnerFlags.NONE, None, None)
        wait(lambda: own_item() is not None, 'host reconnection')
        destination, path = own_item()
        menu_path = prop(destination, path, 'org.kde.StatusNotifierItem', 'Menu')
    menu_click(destination, menu_path, '退出 Hangar')
    wait(lambda: process.poll() is not None, 'explicit quit')
    assert process.returncode == 0
    assert not pathlib.Path(os.environ['CODEX_HOME'], 'auth.json').exists()
    print(f'{mode}: SNI icon/menu, close/hide, show, refresh and quit passed' + (', host loss/reconnect passed' if not args.real_session else ''))
finally:
    if process.poll() is None:
        try: action('quit')
        except GLib.Error: pass
        try: process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.terminate()  # This isolated child only.
            process.wait(timeout=5)
    log.close()
    if owner is not None: Gio.bus_unown_name(owner)
