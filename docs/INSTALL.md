# Installing Burbuja

Download the installer for your system from the [latest release](https://github.com/SantiagoBobrik/burbuja/releases/latest):

| System | File |
|---|---|
| Windows 10 / 11, 64-bit | `Burbuja_<version>_x64-setup.exe` |
| macOS 11 or later, Apple Silicon (M1 and newer) | `Burbuja_<version>_aarch64.dmg` (preview) |

Intel Macs and Linux aren't supported yet. You can still [build Burbuja from source](BUILDING.md).

## Why your computer warns you

Windows and macOS trust an app when its developer pays for a code-signing certificate. Burbuja is a free, open source project and its installers aren't signed yet, so both systems warn you the first time you open it. The warning means the system doesn't know who published the file. It doesn't mean anything harmful was found.

What you can check instead:

- **Where it was built.** Every installer is built by the repository's public [release workflow](../.github/workflows/release.yml) on GitHub's servers, from the commit the release is tagged on. You can read the build log in the [Actions tab](https://github.com/SantiagoBobrik/burbuja/actions/workflows/release.yml).
- **That your file is the one published.** See [Verify your download](#verify-your-download).
- **The code itself.** Everything is in this repository, and you can build it yourself.

## Windows

1. Run `Burbuja_<version>_x64-setup.exe`. If your browser says the file isn't commonly downloaded, choose **Keep**.
2. Windows shows **"Windows protected your PC"**. Click **More info**, then **Run anyway**.
3. Follow the installer. Burbuja then starts, and its icon appears in the taskbar's hidden icons (**^**).

To keep the icon in view, drag it from **^** onto the taskbar.

## macOS

> [!NOTE]
> The macOS version is a preview: it hasn't been tested on a real Mac yet. If something doesn't work, please [open an issue](https://github.com/SantiagoBobrik/burbuja/issues).

1. Open `Burbuja_<version>_aarch64.dmg` and drag **Burbuja** into **Applications**. Run it from Applications, not from the disk image.
2. Open Burbuja. macOS says it can't verify the app. Close the message without moving the app to the Trash.
3. Allow it:
   - **macOS 15 Sequoia and later:** open **System Settings → Privacy & Security**, scroll down to the message about Burbuja and click **Open Anyway**. Confirm with your password.
   - **macOS 14 Sonoma and earlier:** in Finder, Control-click Burbuja in Applications, choose **Open**, then **Open** again.

You only do this once per version.

If macOS says Burbuja **"is damaged and can't be opened"**, the download was quarantined. Run this in Terminal, then open Burbuja again:

```sh
xattr -dr com.apple.quarantine /Applications/Burbuja.app
```

### Permissions

Burbuja's first screen asks for three permissions. Each **Allow** button opens the right place in System Settings:

| Permission | Why |
|---|---|
| Microphone | To hear what you dictate. Audio is processed on your Mac and never leaves it. |
| Accessibility | To catch your shortcut in any app and paste the text where your cursor is. |
| Input Monitoring | Only if your Mac asks for it to catch the shortcut. |

**After updating Burbuja**, macOS treats the new version as a different app, because it isn't signed. If the shortcut or the microphone stops working although the permissions look on, reset them and allow them again:

```sh
tccutil reset Accessibility com.burbuja.app
tccutil reset ListenEvent com.burbuja.app
tccutil reset Microphone com.burbuja.app
```

## Verify your download

Each release includes `SHA256SUMS.txt`, with the SHA-256 checksum of every installer. Compute the checksum of the file you downloaded and check that it matches the line for that file.

Windows (PowerShell, in your Downloads folder):

```powershell
Get-FileHash .\Burbuja_<version>_x64-setup.exe
```

macOS (Terminal, in your Downloads folder):

```sh
shasum -a 256 Burbuja_<version>_aarch64.dmg
```

If the checksums don't match, don't run the file. Download it again from the release page, and if it still doesn't match, [report it privately](../SECURITY.md).

## Uninstall

- **Windows:** Settings → Apps → Installed apps → Burbuja → Uninstall.
- **macOS:** quit Burbuja from its menu bar icon and move it from Applications to the Trash.

Uninstalling keeps your settings, history and speech models. To remove them too, delete these folders:

| System | Folders |
|---|---|
| Windows | `%APPDATA%\com.burbuja.app` and `%LOCALAPPDATA%\com.burbuja.app` |
| macOS | `~/Library/Application Support/com.burbuja.app` and `~/Library/Logs/com.burbuja.app` |
