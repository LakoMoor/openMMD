Independent MMD-compatible editor built with Rust and wgpu. This is a development preview, not a complete 1:1 port of MikuMikuDance 9.32.

Unzip the archive and run `openmmd` (`openmmd.exe` on Windows). Models and textures are not bundled. Select your local PMD/PMX/VMD/VPD/PMM files from the application. Full PMM behavior, physics, accessories, MME effects and visual parity remain incomplete. Keep original project files.

The archive name includes its Rust target architecture. Builds are produced on GitHub-hosted runners for Windows, Linux and macOS; these packages are unsigned. macOS packages currently target Apple Silicon. Linux requires a graphical desktop and a working Vulkan/OpenGL driver plus the platform libraries listed in the README. Build success does not certify GPU runtime behavior on Windows or Linux.

Each ZIP has a companion SHA-256 checksum file. The browser build is published separately at https://lakomoor.github.io/openMMD/app/.

Reference behavior and validation boundaries: https://github.com/LakoMoor/openMMD/blob/main/docs/parity.md.
