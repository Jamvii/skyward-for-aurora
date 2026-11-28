# Changelog

## Unreleased

- The AERA reload button navigates to the last loaded URL, so it works even
  when WebKit ignores a bare Reload action.
- Leaving full mode closes the AERA worker immediately; the next full-mode
  entry starts a fresh browser. Active AERA downloads are marked cancelled.
- The browser menu's full-mode entry opens the AERA start page instead of
  reusing the last URL. The reader's own full-mode link still hands over the
  page being read. Leaving full mode clears the address and loaded URL.
- AERA full mode now loads a small WPE policy hook inside the jail:
  `target="_blank"` / `window.open` navigations stay in the same view, and
  non-renderable responses (APK/ZIP/IMG) go to WebKit's download path.
- The AERA worker's frames are coalesced to the newest frame, acknowledged
  without waiting for a slow draw, and copied into one reusable buffer.
- Browser frame updates redraw only the viewport rows instead of the whole
  page.
- Zip-based AERA runtimes have the worker and WebKit helper executables
  repaired after extraction; download directories use the sticky bit.

## 0.3.5 — Aurora

Skyward 0.3.5 is the first release with the AERA WPE browser as the basic
browser's full mode. AERA is explicitly a test feature: some pages can still
be slow or fail, and the KMS overlay path is not enabled.

### Basic browser

- The Home entry is **浏览器** and opens the basic reader.
- Restores the 0.3.4 reader: text, headings, lists, quotes, code blocks,
  links, pictures, basic CSS, forms, cookies and tabs.
- Restores the start-page shortcuts (Bing, Baidu, GitHub).
- Basic reader downloads now go into the independent download manager.
- The reader is no longer AERA, and Advanced no longer has a Browser entry.

### Full mode — AERA WPE (testing)

- Browser menu → **完整模式（AERA，测试中）** opens AERA at the current URL.
- AERA is marked **测试中，可能不稳定** in the basic browser.
- AERA uses its own WPE WebKit worker, Mesa zink/freedreno and KGSL; the
  runtime stays outside the recovery partition.
- AERA downloads and side-download links are managed by the same download
  manager as the basic reader.
- The KMS overlay experiment is disabled. Browser frames use the stable CPU
  page path; the AERA worker still renders with the GPU.

### Downloads

- Basic reader, AERA and custom links share one manager.
- Cancel, retry, progress and source labels are available in the manager.
- AERA downloads go to `/sdcard/AERA/Downloads`.
- Basic reader downloads go to `/sdcard/Download/Skyward`.

### Fixes and stability

- Basic reader pages can draw their long-list rows again; scrolling no longer
  panics in `pages::row`.
- PNG dimensions are checked before decoding, so a small PNG cannot request a
  huge allocation.
- A page panic is caught and shown as a failed page instead of restarting the
  whole recovery.
- The normal page path uses XBGR without overlay alpha/blend parameters.

### AERA extension package

The release asset `skyward-aera-extension-0.3.5.zip` is the AERA runtime as a
directly usable extension. It contains `aera-web/` and is installed by
Skyward automatically.

The old `aera-runtime.tar.gz` is still accepted.

## Enabling AERA

1. Flash the 0.3.5 recovery image:

   ```sh
   fastboot flash recovery skyward-v0.3.5-aurora.img
   fastboot reboot recovery
   ```

2. Push the extension zip:

   ```sh
   adb push skyward-aera-extension-0.3.5.zip /sdcard/aera-runtime.zip
   ```

3. Connect WLAN and open **浏览器** on Home.

4. Open the browser menu and choose **完整模式（AERA，测试中）**.

5. Skyward unpacks the zip into `/tmp/aera-web` on the first full-mode open.
   The unpack is kept until the next recovery restart; `/tmp` is tmpfs.

If the automatic unpack fails, unpack it by hand:

```sh
adb shell "rm -rf /tmp/aera-web"
adb shell "unzip -q /sdcard/aera-runtime.zip -d /tmp"
```

The worker is `/tmp/aera-web/usr/bin/aera-browser-worker`.

### Requirements and limits

- Data must be decrypted before full mode opens.
- The recovery partition has no room for the roughly 68 MB runtime; the zip
  stays on internal storage.
- AERA is a test feature. Pages with JavaScript-heavy content may be slow or
  fail; the basic reader remains the stable path.
- AERA's complete seccomp policy is not ported; the jail currently uses a
  permissive filter. Do not treat it as a hardened production sandbox.
- The full mode is not the old Chromium plugin. The Chromium plugin and its log
  entry are no longer used.
- KMS browser overlay remains experimental and disabled in this release.

## 0.3.4 and earlier

See the 0.3.4 browser notes and the upstream Skyward changelog for the earlier
reader, WLAN, decryption and install work.
