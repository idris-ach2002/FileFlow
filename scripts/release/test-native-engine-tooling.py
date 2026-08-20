#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import importlib.util
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]


def load(name: str, relative: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    if spec is None or spec.loader is None:
        raise RuntimeError(relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


hardener = load("fileflow_hardener", "scripts/release/harden-engine-pack.py")
validator = load("fileflow_validator", "scripts/release/validate-engine-pack.py")
factory = load("fileflow_factory", "scripts/release/build-native-engine-pack.py")
packer = load("fileflow_packer", "scripts/release/make-engine-pack.py")


class NativeEngineToolingTests(unittest.TestCase):
    def test_vdso_is_not_a_missing_file_but_real_missing_library_is(self):
        output = """
            linux-vdso.so.1 (0x0000)
            libfreetype-fileflow.so.6 => not found
            libc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x0000)
        """
        self.assertTrue(validator.is_linux_virtual_dependency("linux-vdso.so.1 (0x123)"))
        self.assertEqual(validator.parse_ldd_unresolved(output), ["libfreetype-fileflow.so.6"])

    def test_linux_relocator_preserves_wheel_and_plugin_origin_paths(self):
        with tempfile.TemporaryDirectory() as tmp:
            engine_root = Path(tmp) / "engines"
            old = hardener.ENGINE_ROOT
            hardener.ENGINE_ROOT = engine_root
            try:
                pillow = engine_root / "share/runtime/lib/python3.12/site-packages/PIL/_imaging.so"
                pillow_lib = engine_root / "share/runtime/lib/python3.12/site-packages/pillow.libs/libfreetype-fileflow.so.6"
                pike = engine_root / "share/runtime/lib/python3.12/site-packages/pikepdf/_core.so"
                pike_lib = engine_root / "share/runtime/lib/python3.12/site-packages/pikepdf.libs/libqpdf-fileflow.so.30"
                dot = engine_root / "share/runtime/bin/dot_builtins"
                graphviz = engine_root / "share/runtime/lib/graphviz/libgvplugin_core.so.8"
                for path in (pillow, pillow_lib, pike, pike_lib, dot, graphviz):
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(b"fixture")

                cases = [
                    (pillow, "$ORIGIN/../pillow.libs:/opt/build-host", pillow_lib),
                    (pike, "$ORIGIN/../pikepdf.libs", pike_lib),
                    (dot, "$ORIGIN/../lib/graphviz", graphviz),
                ]
                for source, raw_rpath, dependency in cases:
                    entries = hardener.safe_existing_linux_rpaths(source, raw_rpath)
                    self.assertFalse(any(entry.startswith("/opt") for entry in entries))
                    resolved = hardener.resolve_linux_needed(source, dependency.name, [dependency], entries)
                    self.assertEqual(resolved, dependency)
                    derived = hardener.origin_to(source, dependency.parent)
                    self.assertIsNotNone(hardener.expand_origin(source, derived))
            finally:
                hardener.ENGINE_ROOT = old

    def test_certification_seed_never_arch_checks_script_launcher(self):
        with tempfile.TemporaryDirectory() as tmp:
            engine_root = Path(tmp) / "engines"
            native = engine_root / "native.so"
            script = engine_root / "soffice"
            engine_root.mkdir(parents=True)
            native.write_bytes(b"\x7fELFfixture")
            script.write_text("#!/bin/sh\n")
            old_root = validator.ENGINE_ROOT
            validator.ENGINE_ROOT = engine_root
            try:
                with mock.patch.object(validator, "wrapper_targets", return_value=[native, script]), \
                     mock.patch.object(validator, "is_native", side_effect=lambda path, family: path == native):
                    self.assertEqual(validator.certification_seed([native, script], "linux"), [native])
            finally:
                validator.ENGINE_ROOT = old_root

    def test_linux_libreoffice_host_dependency_is_vendored(self):
        with tempfile.TemporaryDirectory() as tmp:
            pack = Path(tmp) / "pack"
            solver = pack / "share/libreoffice/program/libsolverlo.so"
            solver.parent.mkdir(parents=True)
            solver.write_bytes(b"\x7fELFsolver")
            host = Path(tmp) / "host/liblpsolve55.so"
            host.parent.mkdir(parents=True)
            host.write_bytes(b"\x7fELFlpsolve")

            def needed(path):
                if path.name == "libsolverlo.so":
                    return ["liblpsolve55.so", "libc.so.6"]
                return ["libc.so.6"]

            with mock.patch.object(factory, "linux_needed", side_effect=needed), \
                 mock.patch.object(factory, "linux_host_library_index", return_value={}), \
                 mock.patch.object(factory, "find_linux_host_library", return_value=host):
                factory.vendor_linux_libreoffice_dependencies(pack)

            copied = pack / "share/runtime/lib/liblpsolve55.so"
            self.assertTrue(copied.is_file())
            self.assertEqual(copied.read_bytes(), host.read_bytes())

    def test_linux_ambiguous_internal_dependency_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            engine_root = Path(tmp) / "engines"
            old = hardener.ENGINE_ROOT
            hardener.ENGINE_ROOT = engine_root
            try:
                source = engine_root / "share/runtime/bin/tool"
                one = engine_root / "share/runtime/a/libdup.so"
                two = engine_root / "share/runtime/b/libdup.so"
                for path in (source, one, two):
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(b"fixture")
                with self.assertRaises(SystemExit):
                    hardener.resolve_linux_needed(source, "libdup.so", [one, two], [])
            finally:
                hardener.ENGINE_ROOT = old

    def test_macos_absolute_libreoffice_path_maps_to_copied_tree(self):
        with tempfile.TemporaryDirectory() as tmp:
            engine_root = Path(tmp) / "engines"
            old = hardener.ENGINE_ROOT
            hardener.ENGINE_ROOT = engine_root
            try:
                source = engine_root / "share/libreoffice/Contents/MacOS/soffice"
                target = engine_root / "share/libreoffice/Contents/Frameworks/libuno.dylib"
                for path in (source, target):
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(b"fixture")
                dep = "/Applications/LibreOffice.app/Contents/Frameworks/libuno.dylib"
                self.assertEqual(hardener.macos_absolute_pack_candidate(dep, [target]), target)
            finally:
                hardener.ENGINE_ROOT = old

    def test_macos_ambiguous_basename_is_never_chosen_arbitrarily(self):
        with tempfile.TemporaryDirectory() as tmp:
            engine_root = Path(tmp) / "engines"
            old = hardener.ENGINE_ROOT
            hardener.ENGINE_ROOT = engine_root
            try:
                source = engine_root / "share/runtime/bin/tool"
                one = engine_root / "share/runtime/lib/a/libsame.dylib"
                two = engine_root / "share/libreoffice/program/libsame.dylib"
                for path in (source, one, two):
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(b"fixture")
                with self.assertRaises(SystemExit):
                    hardener.resolve_macos_dependency(source, "@rpath/libsame.dylib", [one, two], [])
            finally:
                hardener.ENGINE_ROOT = old

    def test_wrappers_scrub_build_host_and_chain_through_fileflow_bin(self):
        wrapper = factory.unix_wrapper("share/runtime/bin/python", ["-m", "ocrmypdf"])
        self.assertIn('PATH="$BIN_DIR:$RUNTIME/bin:', wrapper)
        self.assertIn("unset CONDA_PREFIX", wrapper)
        self.assertIn("PYTHONNOUSERSITE=1", wrapper)
        self.assertNotIn("${PATH:-}", wrapper)
        self.assertIn('engine-runtime-paths.txt', factory.WINDOWS_LAUNCHER)
        self.assertIn('MAGICK_CONFIGURE_PATH', factory.WINDOWS_LAUNCHER)
        self.assertIn('GS_LIB', factory.WINDOWS_LAUNCHER)
        self.assertIn('remove_var("CONDA_PREFIX")', factory.WINDOWS_LAUNCHER)

    def test_engine_archive_is_reproducible_for_identical_content(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "pack"
            (root / "bin").mkdir(parents=True)
            executable = root / "bin" / "tool"
            executable.write_text("#!/bin/sh\nexit 0\n")
            executable.chmod(0o755)
            first = Path(tmp) / "one.tar.gz"
            second = Path(tmp) / "two.tar.gz"
            packer.write_reproducible_tar_gz(root, first, "pack")
            packer.write_reproducible_tar_gz(root, second, "pack")
            self.assertEqual(hashlib.sha256(first.read_bytes()).digest(), hashlib.sha256(second.read_bytes()).digest())


if __name__ == "__main__":
    unittest.main(verbosity=2)
