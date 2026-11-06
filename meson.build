project('proton-pass-gtk', version: '0.2.0', meson_version: '>= 0.63.0', license: 'GPL-3.0-or-later')
app_id = 'io.github.isyourbrainfoss.ProtonPassGtk'
dependency('gtk4', version: '>= 4.12')
dependency('libadwaita-1', version: '>= 1.5')
cargo = find_program('cargo')
python = find_program('python3')
subdir('data')
rust_app = custom_target('proton-pass-gtk',
  output: 'proton-pass-gtk',
  command: [python, files('build-aux/cargo-build.py'), meson.project_source_root(), meson.project_build_root(), '@OUTPUT@'],
  build_always_stale: true,
  build_by_default: true,
  install: true,
  install_dir: get_option('bindir'))
test('cargo-tests', cargo, args: ['test', '--locked', '--offline', '--manifest-path', meson.project_source_root() / 'Cargo.toml'],
  env: ['CARGO_TARGET_DIR=' + meson.project_build_root() / 'cargo-target'], timeout: 300)
