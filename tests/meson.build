pytest = find_program('pytest-3', 'pytest', required: false)
if pytest.found()
  test('pytest', pytest,
    args: ['-q', meson.current_source_dir()],
    workdir: meson.project_source_root(),
  )
endif
