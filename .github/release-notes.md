## Download

| System | File |
|---|---|
| Windows 10 / 11, 64-bit | `Burbuja_{{VERSION}}_x64-setup.exe` |
| macOS 11 or later, Apple Silicon (M1 and newer) | `Burbuja_{{VERSION}}_aarch64.dmg` (preview) |

> [!IMPORTANT]
> The installers aren't code-signed yet, so Windows and macOS warn you the first time you open Burbuja. **[Installing Burbuja](https://github.com/{{REPO}}/blob/v{{VERSION}}/docs/INSTALL.md)** walks you through each warning.

The installers are built in public by [this repository's release workflow](https://github.com/{{REPO}}/blob/v{{VERSION}}/.github/workflows/release.yml), from the commit this release is tagged on.

### Verify your download

`SHA256SUMS.txt` lists the SHA-256 checksum of every installer. Compute yours and check that it matches:

- Windows (PowerShell): `Get-FileHash .\Burbuja_{{VERSION}}_x64-setup.exe`
- macOS (Terminal): `shasum -a 256 Burbuja_{{VERSION}}_aarch64.dmg`

## What's new

<!-- Write the changes here before publishing. -->
