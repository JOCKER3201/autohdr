#!/bin/bash

# AutoHDR Installation Script
# This script performs a clean build and installs the Vulkan layer and GUI.

set -e # Exit on error

echo "[AutoHDR] Cleaning up old build artifacts..."
rm -rfv build
rm -rfv target
rm -rfv autohdr-gui/target

echo "[AutoHDR] Configuring build with Meson..."
meson build --prefix=/usr/local

echo "[AutoHDR] Compiling..."
ninja -C build -j16

echo "[AutoHDR] Installing (requires sudo)..."
sudo ninja -C build install

echo "[AutoHDR] Post-installation cleanup..."
rm -rfv build
rm -rfv target
rm -rfv autohdr-gui/target

echo "[AutoHDR] Installation complete! You can now run 'autohdr-gui' or use the layer in games."
