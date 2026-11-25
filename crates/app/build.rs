//! Ícono y datos del programa en el .exe de Windows (los que se ven en el Explorador y la barra).
fn main() {
    println!("cargo:rerun-if-changed=../../installer/icon.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../installer/icon.ico")
            .set("ProductName", "Smurf Vault")
            .set("FileDescription", "Smurf Vault")
            .set("CompanyName", "tobaalhs")
            .set("LegalCopyright", "tobaalhs");
        if let Err(e) = res.compile() {
            println!("cargo:warning=no pude poner el ícono al .exe: {e}");
        }
    }
}
