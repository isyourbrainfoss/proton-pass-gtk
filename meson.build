project('proton-pass-gtk',
  version: '0.1.0',
  meson_version: '>= 0.62.0',
  license: 'GPL-3.0-or-later',
)

app_id = 'io.github.isyourbrainfoss.ProtonPassGtk'

python = import('python')
py = python.find_installation('python3')

# Pure Python app: only the runtime typelibs are needed, not -dev packages.
gi_check = run_command(py, '-c',
  'import gi; gi.require_version("Gtk", "4.0"); gi.require_version("Adw", "1"); ' +
  'from gi.repository import Gtk, Adw; ' +
  'assert (Gtk.get_major_version(), Gtk.get_minor_version()) >= (4, 12); ' +
  'assert (Adw.get_major_version(), Adw.get_minor_version()) >= (1, 5)',
  check: false)
if gi_check.returncode() != 0
  warning('PyGObject with GTK >= 4.12 and libadwaita >= 1.5 not found at build time')
endif

prefix = get_option('prefix')
pkgdatadir = prefix / get_option('datadir') / meson.project_name()
moduledir = py.get_install_dir() / 'protonpassgtk'

subdir('src')
subdir('data')
subdir('tests')

gnome = import('gnome')
gnome.post_install(
  gtk_update_icon_cache: true,
  update_desktop_database: true,
)
