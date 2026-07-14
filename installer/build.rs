fn main() {
    // In build scripts cfg(windows) is the HOST, but this exe is only ever built on
    // windows-latest in CI (and checked there via windows-check.yml), so host == target.
    #[cfg(windows)]
    {
        let mut res = tauri_winres::WindowsResource::new();
        res.set_icon("../src-tauri/icons/icon.ico");
        // requireAdministrator: the MSI installs per-machine, so elevate the whole
        // bootstrapper up front (one UAC prompt at launch, standard for a setup.exe)
        // instead of msiexec failing silently mid-flow.
        res.set_manifest(
            r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>"#,
        );
        res.compile().expect("failed to compile Windows resources");
    }
}
