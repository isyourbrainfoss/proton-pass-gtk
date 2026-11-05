py.install_sources(
  'protonpassgtk/__init__.py',
  'protonpassgtk/__main__.py',
  'protonpassgtk/clipboard.py',
  'protonpassgtk/item_view.py',
  'protonpassgtk/login.py',
  'protonpassgtk/main.py',
  'protonpassgtk/models.py',
  'protonpassgtk/parsing.py',
  'protonpassgtk/widgets.py',
  'protonpassgtk/window.py',
  subdir: 'protonpassgtk',
)
py.install_sources(
  'protonpassgtk/backend/__init__.py',
  'protonpassgtk/backend/argv.py',
  'protonpassgtk/backend/base.py',
  'protonpassgtk/backend/mock.py',
  'protonpassgtk/backend/passcli.py',
  subdir: 'protonpassgtk/backend',
)

conf = configuration_data()
conf.set('PYTHON', py.full_path())
conf.set('pythondir', py.get_install_dir())
configure_file(
  input: 'proton-pass-gtk.in',
  output: 'proton-pass-gtk',
  configuration: conf,
  install: true,
  install_dir: get_option('bindir'),
  install_mode: 'rwxr-xr-x',
)
