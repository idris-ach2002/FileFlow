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
functional = load("fileflow_functional", "scripts/release/functional-engine-tests.py")
libreoffice_source = load("fileflow_libreoffice_source", "scripts/release/fetch-libreoffice-runtime.py")


class NativeEngineToolingTests(unittest.TestCase):

    def test_official_libreoffice_recipe_covers_all_certified_targets(self):
        manifest = __import__("json").loads((ROOT / "release/engines/libreoffice-runtime.json").read_text())
        expected = {
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-pc-windows-msvc",
        }
        self.assertEqual(set(manifest["targets"]), expected)
        for entry in manifest["targets"].values():
            self.assertTrue(entry["url"].startswith("https://download.documentfoundation.org/"))

    def test_official_libreoffice_sha_sidecar_must_match_pin(self):
        sha = "a" * 64
        with mock.patch.object(libreoffice_source, "fetch_text", return_value=f"{sha}  LibreOffice.bin\n"):
            self.assertEqual(libreoffice_source.official_sha256("https://example.invalid/LibreOffice.bin", sha), sha)
            with self.assertRaises(SystemExit):
                libreoffice_source.official_sha256("https://example.invalid/LibreOffice.bin", "b" * 64)

    def test_office_wrapper_does_not_preload_private_library_namespace(self):
        wrapper = factory.unix_wrapper("share/libreoffice/program/soffice", office=True)
        office_branch = wrapper.split('if [ "1" = "1" ]; then', 1)[1].split("else", 1)[0]
        self.assertIn("unset PYTHONHOME PYTHONPATH LD_LIBRARY_PATH DYLD_LIBRARY_PATH", office_branch)
        self.assertNotIn("export LD_LIBRARY_PATH", office_branch)
        self.assertNotIn("export DYLD_LIBRARY_PATH", office_branch)

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
                 mock.patch.object(factory, "elf_machine", return_value=62), \
                 mock.patch.object(factory, "linux_host_library_index", return_value={}), \
                 mock.patch.object(factory, "find_linux_host_library", return_value=host):
                factory.vendor_linux_libreoffice_dependencies(pack)

            copied = pack / "share/libreoffice/lib/liblpsolve55.so"
            self.assertTrue(copied.is_file())
            self.assertEqual(copied.read_bytes(), host.read_bytes())

    def test_linux_provider_selection_rejects_wrong_architecture(self):
        with tempfile.TemporaryDirectory() as tmp:
            wrong = Path(tmp) / "i386/libsample.so.1"
            right = Path(tmp) / "x64/libsample.so.1"
            wrong.parent.mkdir(parents=True)
            right.parent.mkdir(parents=True)
            wrong.write_bytes(b"wrong")
            right.write_bytes(b"right")
            index = {"libsample.so.1": [wrong, right]}
            with mock.patch.object(factory, "elf_machine", side_effect=lambda path: 62 if Path(path).resolve(strict=False) == right.resolve(strict=False) else 3):
                selected = factory.find_linux_host_library(
                    "libsample.so.1", index, "x86_64-unknown-linux-gnu"
                )
            self.assertEqual(selected, right)

    def test_macos_external_dependency_is_copied_into_private_vendor(self):
        with tempfile.TemporaryDirectory() as tmp:
            pack = Path(tmp) / "pack"
            source = pack / "share/runtime/bin/tool"
            host = Path(tmp) / "host/liboutside.dylib"
            source.parent.mkdir(parents=True)
            host.parent.mkdir(parents=True)
            source.write_bytes(b"mach-source")
            host.write_bytes(b"mach-host")
            with mock.patch.object(factory, "macos_is_native", return_value=True), \
                 mock.patch.object(factory, "macos_dependencies", side_effect=lambda path: ["/opt/vendor/liboutside.dylib"] if path == source else []), \
                 mock.patch.object(factory, "find_macos_host_dependency", return_value=host):
                records = factory.vendor_macos_external_dependencies(
                    pack, "aarch64-apple-darwin", Path(tmp) / "conda"
                )
            copied = pack / "share/vendor/macos/liboutside.dylib"
            self.assertTrue(copied.is_file())
            self.assertEqual(len(records), 1)

    def test_windows_external_dependency_is_copied_into_private_vendor(self):
        with tempfile.TemporaryDirectory() as tmp:
            pack = Path(tmp) / "pack"
            source = pack / "share/runtime/bin/tool.exe"
            prefix = Path(tmp) / "conda"
            host = prefix / "Library/bin/thirdparty.dll"
            source.parent.mkdir(parents=True)
            host.parent.mkdir(parents=True)
            source.write_bytes(b"pe-source")
            host.write_bytes(b"pe-host")
            def machine(path):
                return 0x8664 if Path(path) in {source, host, pack / "share/vendor/windows/thirdparty.dll"} else None
            with mock.patch.object(factory, "pe_machine", side_effect=machine), \
                 mock.patch.object(factory, "pe_imports", side_effect=lambda path: ["thirdparty.dll"] if path == source else []), \
                 mock.patch.object(factory, "windows_host_dll_index", return_value={"thirdparty.dll": [host]}):
                records = factory.vendor_windows_external_dependencies(
                    pack, "x86_64-pc-windows-msvc", prefix
                )
            copied = pack / "share/vendor/windows/thirdparty.dll"
            self.assertTrue(copied.is_file())
            self.assertEqual(len(records), 1)


    def test_linux_libreoffice_prefers_isolated_distro_closure_over_conda_duplicate(self):
        with tempfile.TemporaryDirectory() as tmp:
            engine_root = Path(tmp) / "engines"
            old = hardener.ENGINE_ROOT
            hardener.ENGINE_ROOT = engine_root
            try:
                source = engine_root / "share/libreoffice/program/soffice.bin"
                office_dep = engine_root / "share/libreoffice/lib/libsame.so.1"
                conda_dep = engine_root / "share/runtime/lib/libsame.so.1"
                for path in (source, office_dep, conda_dep):
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(b"fixture")
                resolved = hardener.resolve_linux_needed(
                    source, "libsame.so.1", [conda_dep, office_dep], []
                )
                self.assertEqual(resolved, office_dep)
            finally:
                hardener.ENGINE_ROOT = old

    def test_libreoffice_wrapper_does_not_inherit_conda_loader_namespace(self):
        wrapper = factory.unix_wrapper("share/libreoffice/program/soffice", office=True)
        self.assertIn('if [ "1" = "1" ]', wrapper)
        self.assertIn("unset PYTHONHOME PYTHONPATH LD_LIBRARY_PATH DYLD_LIBRARY_PATH", wrapper)
        self.assertIn("SAL_USE_VCLPLUGIN=svp", wrapper)

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


    def test_img2pdf_fixture_is_opaque_and_large_enough_for_pdf(self):
        with tempfile.TemporaryDirectory() as tmp:
            image = Path(tmp) / "fixture.png"
            functional.write_rgb_png(image)
            data = image.read_bytes()
            self.assertEqual(data[:8], b"\x89PNG\r\n\x1a\n")
            width = int.from_bytes(data[16:20], "big")
            height = int.from_bytes(data[20:24], "big")
            color_type = data[25]
            self.assertGreaterEqual(width, 32)
            self.assertGreaterEqual(height, 32)
            self.assertEqual(color_type, 2)  # RGB, no alpha channel

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
