#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
import os, pathlib, shutil, subprocess, sys
source, build, output = map(pathlib.Path, sys.argv[1:])
env = os.environ.copy()
env["CARGO_TARGET_DIR"] = str(build / "cargo-target")
subprocess.run(["cargo", "build", "--release", "--locked", "--offline", "--manifest-path", str(source / "Cargo.toml")], env=env, check=True)
shutil.copy2(build / "cargo-target/release/proton-pass-gtk", output)
