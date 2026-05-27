#!/usr/bin/env bash
#
# Run headless Godot, tolerating the known native-GDExtension teardown crash.
#
# The webrtc-native GDExtension (libdatachannel/libjuice background threads)
# segfaults Godot on EXIT in headless mode — but only *after* all work
# (import / export) has completed and the artifacts are written. We forgive
# SIGSEGV (139) and SIGABRT (134) at exit and rely on the artifact-verification
# steps that follow each export (pck size, per-arch managed DLL, native-lib
# presence) to catch any genuinely incomplete output.
#
# This is CI-only. We deliberately keep the extension's editor library so that
# local in-editor play (F5, which reports OS.has_feature("editor")) still has
# WebRTC. See docs/planning/multiplayer-client-stages.md.
set -uo pipefail

godot "$@"
code=$?

if [ "$code" -eq 0 ]; then
  exit 0
fi
if [ "$code" -eq 139 ] || [ "$code" -eq 134 ]; then
  echo "::warning::godot $* exited with $code — tolerating GDExtension teardown crash; output is verified separately."
  exit 0
fi
exit "$code"
