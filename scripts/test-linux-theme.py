#!/usr/bin/env python3
"""Probe Libadwaita system preferences without changing the user's desktop settings.

Default: private Settings portal fixture, emits light/dark changes.
--real-session: empty QA config on the real bus; read-only preference comparison.
"""
import argparse
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time

parser = argparse.ArgumentParser()
parser.add_argument('--child', action='store_true', help=argparse.SUPPRESS)
parser.add_argument('--probe', action='store_true', help=argparse.SUPPRESS)
parser.add_argument('--real-session', action='store_true')
args = parser.parse_args()
if not args.child and not args.probe:
    with tempfile.TemporaryDirectory(prefix='hangar-theme-') as home:
        env = os.environ.copy()
        env.update(HOME=home, CODEX_HOME=home+'/.codex', XDG_CONFIG_HOME=home+'/config',
                   XDG_DATA_HOME=home+'/data', XDG_CACHE_HOME=home+'/cache', GIO_USE_VFS='local')
        cmd = ['/usr/bin/python3', __file__, '--child']
        cmd = cmd+['--real-session'] if args.real_session else ['dbus-run-session', '--']+cmd
        if not args.real_session:
            wayland = env.get('WAYLAND_DISPLAY', '')
            if wayland and not wayland.startswith('/'):
                env['WAYLAND_DISPLAY'] = str(pathlib.Path(env['XDG_RUNTIME_DIR'], wayland))
            env['XDG_RUNTIME_DIR'] = home+'/runtime'
            pathlib.Path(env['XDG_RUNTIME_DIR']).mkdir(mode=0o700)
        try:
            result = subprocess.run(cmd, env=env, timeout=35)
        finally:
            if not args.real_session:
                for name in ('doc', 'gvfs'):
                    subprocess.run(['fusermount3', '-u', '-z', home+'/runtime/'+name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        sys.exit(result.returncode)

import gi
gi.require_version('Adw', '1')
from gi.repository import Gio, GLib
path = pathlib.Path(os.environ['HOME'], 'theme-probe.json')
if args.probe:
    from gi.repository import Adw
    Adw.init()
    style = Adw.StyleManager.get_default()
    style.set_color_scheme(Adw.ColorScheme.PREFER_LIGHT)
    def save(*_):
        temp = path.with_suffix('.tmp')
        temp.write_text(json.dumps({'dark': style.get_dark(), 'supported': style.get_system_supports_color_schemes()}))
        temp.replace(path)
    style.connect('notify::dark', save)
    save()
    GLib.MainLoop().run()
    sys.exit()

bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
portal = 'org.freedesktop.portal.Desktop'
obj = '/org/freedesktop/portal/desktop'
settings = 'org.freedesktop.portal.Settings'
namespace = 'org.freedesktop.appearance'
owner = None
scheme = 1

def tick():
    while GLib.MainContext.default().iteration(False): pass
    time.sleep(.02)

def wait(predicate):
    deadline = time.monotonic()+8
    while time.monotonic() < deadline:
        tick()
        if predicate(): return
    raise AssertionError('theme probe did not follow portal preference')

if not args.real_session:
    xml = '''<node><interface name="org.freedesktop.portal.Settings">
    <property name="version" type="u" access="read"/>
    <method name="ReadOne"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="out"/></method>
    <method name="Read"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="out"/></method>
    <method name="ReadAll"><arg type="as" direction="in"/><arg type="a{sa{sv}}" direction="out"/></method>
    <signal name="SettingChanged"><arg type="s"/><arg type="s"/><arg type="v"/></signal>
    </interface></node>'''
    def method(connection, sender, object_path, interface, name, params, invocation):
        if name == 'ReadAll':
            invocation.return_value(GLib.Variant('(a{sa{sv}})', ({namespace: {'color-scheme': GLib.Variant('u', scheme)}},)))
        elif params.unpack() == (namespace, 'color-scheme'):
            value = GLib.Variant('u', scheme)
            if name == 'Read': value = GLib.Variant('v', value)  # Legacy Read wraps twice.
            invocation.return_value(GLib.Variant('(v)', (value,)))
        else:
            invocation.return_dbus_error('org.freedesktop.portal.Error.NotFound', 'No such setting')
    bus.register_object(obj, Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0], method, lambda *_: GLib.Variant('u', 2), None)
    acquired = []
    owner = Gio.bus_own_name_on_connection(bus, portal, Gio.BusNameOwnerFlags.NONE, lambda *_: acquired.append(True), None)
    wait(lambda: bool(acquired))
else:
    scheme = bus.call_sync(portal, obj, settings, 'Read', GLib.Variant('(ss)', (namespace, 'color-scheme')), None, Gio.DBusCallFlags.NONE, 5000, None).unpack()[0]

probe = subprocess.Popen(['/usr/bin/python3', __file__, '--probe'], stdout=subprocess.DEVNULL)
def state():
    return json.loads(path.read_text()) if path.exists() else {}
try:
    wait(lambda: state().get('supported') and state().get('dark') == (scheme == 1))
    if not args.real_session:
        for scheme in (2, 1, 0):
            bus.emit_signal(None, obj, settings, 'SettingChanged', GLib.Variant('(ssv)', (namespace, 'color-scheme', GLib.Variant('u', scheme))))
            wait(lambda: state().get('dark') == (scheme == 1))
        print('private portal: dark → light → dark → default changes followed')
    else:
        print(f'real desktop with empty QA config: portal scheme={scheme}, Libadwaita dark={state()["dark"]}')
finally:
    probe.terminate()
    probe.wait(timeout=5)
    if owner is not None: Gio.bus_unown_name(owner)
