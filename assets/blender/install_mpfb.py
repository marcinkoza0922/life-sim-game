#!/usr/bin/env python3
"""Installs the pinned MPFB2 extension and MakeHuman CC0 system asset pack that peon_mpfb.py builds
peons from (`PEON_BASE=mpfb` in make_peons.py). Run with plain Python from the repo root:

    python3 assets/blender/install_mpfb.py [--blender blender]

Downloads each file into a temporary directory, checks its SHA-256 against the pins below, installs
the extension into Blender's `blender_org` repository (so it imports as bl_ext.blender_org.mpfb),
unzips the asset pack into MPFB's user data directory and writes a stamp there that peon_mpfb.py
checks before building. To move to a new version, change the pins (and regenerate the models).
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import zipfile

MPFB_VERSION = "2.0.18"
MPFB_URL = ("https://extensions.blender.org/download/sha256:c075b3e9b94b8e5556a5f03f4653602d02ea306de08126e8f55ed6dee28a2dad/"
            "add-on-mpfb-v2.0.18.zip")
MPFB_SHA256 = "c075b3e9b94b8e5556a5f03f4653602d02ea306de08126e8f55ed6dee28a2dad"
ASSETS_URL = "http://files.makehumancommunity.org/asset_packs/makehuman_system_assets/makehuman_system_assets_cc0.zip"
ASSETS_SHA256 = "b542127a8e25547c7c29c19f2d1d2adb9a664c80396ecd694095dbc8028a0107"
STAMP = ".peon_pin.json"

# prints MPFB's user data directory (run inside Blender once the extension is installed)
WHERE = "from bl_ext.blender_org.mpfb.services.locationservice import LocationService as L; print('USERDATA=' + L.get_user_data())"


def fetch(url, sha, path):
    print(f"downloading {url}")
    # curl, not urllib: extensions.blender.org refuses Python's HTTP client (403/466)
    subprocess.run(["curl", "-fsSL", "-o", path, url], check=True)
    h = hashlib.sha256(open(path, "rb").read()).hexdigest()
    if h != sha:
        sys.exit(f"{os.path.basename(path)}: SHA-256 {h}, expected {sha}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--blender", default="blender")
    a = ap.parse_args()
    with tempfile.TemporaryDirectory() as tmp:
        ext = os.path.join(tmp, "mpfb.zip")
        fetch(MPFB_URL, MPFB_SHA256, ext)
        subprocess.run([a.blender, "-b", "-c", "extension", "install-file", "-r", "blender_org", "--enable", ext], check=True)
        out = subprocess.run([a.blender, "-b", "--python-expr", WHERE], check=True, capture_output=True, text=True).stdout
        data = next(line.split("=", 1)[1] for line in out.splitlines() if line.startswith("USERDATA="))
        pack = os.path.join(tmp, "assets.zip")
        fetch(ASSETS_URL, ASSETS_SHA256, pack)
        os.makedirs(data, exist_ok=True)
        with zipfile.ZipFile(pack) as z:
            for name in z.namelist():  # refuse paths that would leave the data directory
                if os.path.isabs(name) or ".." in name.split("/"):
                    sys.exit(f"unsafe path in asset pack: {name}")
            z.extractall(data)
    with open(os.path.join(data, STAMP), "w") as f:
        json.dump({"mpfb": MPFB_VERSION, "assets_sha256": ASSETS_SHA256}, f)
    print(f"installed MPFB {MPFB_VERSION} and the asset pack into {data}")


if __name__ == "__main__":
    main()
